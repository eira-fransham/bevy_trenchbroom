use super::*;
use crate::*;
use bevy_mesh::{Indices, PrimitiveTopology};
use bsp::*;

#[derive(Default)]
pub struct InternalModel {
	pub meshes: Vec<InternalModelMesh>,
	/// Entity to apply [`Brushes`] to. Should probably only be one of these.
	pub entity: Option<Entity>,
}

// We need to run spawners before adding model assets because they have mutable access to meshes
pub struct InternalModelMesh {
	pub texture: MapGeometryTexture,
	pub mesh: Mesh,
	/// Entity to apply [`Mesh3d`] to. Should probably only be one of these.
	pub entity: Option<Entity>,
}

#[inline]
fn convert_vec3(config: &TrenchBroomConfig) -> impl Fn(qbsp::glam::Vec3) -> Vec3 + '_ {
	|x| config.to_bevy_space(Vec3::from_array(x.to_array()))
}

#[cfg(feature = "client")]
type Lightmap = BspLightmap;
#[cfg(not(feature = "client"))]
type Lightmap = LightmapUvMap;

pub async fn compute_models<'a, 'lc: 'a>(
	ctx: &mut BspLoadCtx<'a, 'lc>,
	lightmap: &Option<Lightmap>,
	embedded_textures: &EmbeddedTextures,
) -> Vec<InternalModel> {
	let tb_server = &ctx.loader.tb_server;
	let config = &tb_server.config;
	#[cfg(feature = "client")]
	let lightmap_uvs = lightmap.as_ref().map(|lm| &lm.uv_map);
	#[cfg(not(feature = "client"))]
	let lightmap_uvs = lightmap.as_ref();

	let mut models = Vec::with_capacity(ctx.data.models.len());

	for model_idx in 0..ctx.data.models.len() {
		let model_output = ctx.data.mesh_model(model_idx, lightmap_uvs);
		let mut model = InternalModel::default();
		model.meshes.reserve(model_output.meshes.len());

		for exported_mesh in model_output.meshes {
			let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, config.brush_mesh_asset_usages);

			mesh.insert_attribute(
				Mesh::ATTRIBUTE_POSITION,
				exported_mesh.positions.into_iter().map(convert_vec3(config)).collect_vec(),
			);
			mesh.insert_attribute(
				Mesh::ATTRIBUTE_NORMAL,
				exported_mesh.normals.into_iter().map(convert_vec3(config)).collect_vec(),
			);
			mesh.insert_attribute(
				Mesh::ATTRIBUTE_UV_0,
				exported_mesh.uvs.iter().map(qbsp::glam::Vec2::to_array).collect_vec(),
			);
			if let Some(lightmap_uvs) = &exported_mesh.lightmap_uvs {
				mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, lightmap_uvs.iter().map(qbsp::glam::Vec2::to_array).collect_vec());
			}
			mesh.insert_indices(Indices::U32(exported_mesh.indices.into_flattened()));

			if let Err(err) = mesh.generate_tangents() {
				error!(
					"Failed to generate tangents for model {model_idx}, mesh with texture {}: {err}",
					exported_mesh.texture.as_ref().map(|s| s.as_str()).unwrap_or("{unknown}")
				);
			}

			let texture_name = exported_mesh.texture.as_ref().map(|s| s.as_str());

			let material = match exported_mesh
				.texture
				.as_ref()
				.and_then(|tex| embedded_textures.textures.get(tex.as_str()))
			{
				Some(embedded_texture) => embedded_texture.material.clone(),
				None => {
					(config.load_loose_texture)(TextureLoadView {
						name: texture_name.unwrap_or(""),
						tb_server,
						load_context: ctx.load_context,
						asset_server: ctx.asset_server,
						entities: ctx.entities,
						#[cfg(feature = "client")]
						alpha_mode: None,
						embedded_textures: Some(&embedded_textures.images),
					})
					.await
				}
			};

			model.meshes.push(InternalModelMesh {
				texture: MapGeometryTexture {
					material,
					#[cfg(feature = "client")]
					lightmap: lightmap.as_ref().map(|lm| lm.animated_lighting.clone()),
					name: texture_name.map(|s| s.to_string()),
					flags: exported_mesh.tex_flags,
				},
				mesh,
				entity: None,
			});
		}

		models.push(model)
	}

	models
}

pub fn finalize_models(ctx: &mut BspLoadCtx, models: Vec<InternalModel>) -> anyhow::Result<Vec<BspModel>> {
	let config = &ctx.loader.tb_server.config;

	Ok(models
		.into_iter()
		.enumerate()
		.map(|(model_idx, model)| BspModel {
			meshes: model
				.meshes
				.into_iter()
				.enumerate()
				.map(|(mesh_idx, model_mesh)| {
					let mesh_handle = ctx
						.load_context
						.add_labeled_asset(format!("Model{model_idx}Mesh{mesh_idx}"), model_mesh.mesh);

					BspMesh {
						name: model_mesh.texture.name.unwrap_or_default(),
						material: model_mesh.texture.material.clone(),
						lightmap: model_mesh.texture.lightmap.clone(),
						mesh: mesh_handle,
					}
				})
				.collect(),

			brushes: Default::default(),
		})
		.collect())
}
