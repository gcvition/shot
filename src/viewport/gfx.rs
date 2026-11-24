//! 渲染辅助：房间网格、材质、2D 网格拼装。纯「怎么画」，不含玩法逻辑。

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::appearance;
use crate::theme::Theme;

pub(crate) fn six_target_mat() -> StandardMaterial {
    // Sixshot：很小的黑点，不吃光照，避免高光糊成一团。
    StandardMaterial {
        base_color: Color::srgb(0.02, 0.02, 0.022),
        unlit: true,
        ..default()
    }
}

pub(crate) fn grid_target_mat(theme: &Theme) -> StandardMaterial {
    // Gridshot：主题色金属球，粗糙度压低才会「亮」。
    let rgb = theme.enemy_rgb();
    StandardMaterial {
        base_color: Color::srgb(rgb[0].max(0.012), rgb[1].max(0.012), rgb[2].max(0.016)),
        perceptual_roughness: theme.enemy_roughness().clamp(0.06, 0.18),
        metallic: theme.enemy_metallic().max(0.78),
        reflectance: 0.62,
        ..default()
    }
}

fn unlit_tiled(
    texture: Handle<Image>,
    face_w: f32,
    face_h: f32,
    tile_m: f32,
    tint: Color,
) -> StandardMaterial {
    StandardMaterial {
        base_color: tint,
        base_color_texture: Some(texture),
        unlit: true,
        uv_transform: Affine2::from_scale(Vec2::new(
            (face_w / tile_m).max(0.5),
            (face_h / tile_m).max(0.5),
        )),
        ..default()
    }
}

/// 用主题色生成大理石墙 + 地砖，拼出一个封闭房间。目标贴在 `WALL_Z` 那面墙上。
pub(crate) fn spawn_kovaak_room(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    theme: &Theme,
    layer: &RenderLayers,
) {
    let wall = theme.wall_rgb();
    let floor = theme.floor_rgb();
    let marble = images.add(rgba_to_image(appearance::marble_texture(wall, 512), false));
    let paver = images.add(rgba_to_image(appearance::brick_texture(floor, 512), false));

    let half = appearance::ROOM_HALF_X;
    let height = appearance::ROOM_HEIGHT;
    let thick = appearance::WALL_THICK;
    let front = appearance::room_front();
    let back = appearance::ROOM_BACK;
    let depth = (back - front).abs().max(0.5);
    let z_mid = (front + back) * 0.5;
    let tile = appearance::MARBLE_TILE_M;

    let floor_mat = materials.add(unlit_tiled(
        paver,
        half * 2.0,
        depth,
        appearance::PAVER_TILE_M,
        Color::WHITE,
    ));
    let wall_front = materials.add(unlit_tiled(
        marble.clone(),
        half * 2.0,
        height,
        tile,
        Color::WHITE,
    ));
    let wall_side = materials.add(unlit_tiled(
        marble.clone(),
        depth,
        height,
        tile,
        Color::WHITE,
    ));
    let ceil_mat = materials.add(unlit_tiled(
        marble.clone(),
        half * 2.0,
        depth,
        tile,
        Color::srgb(0.88, 0.88, 0.90),
    ));
    let ledge_mat = materials.add(unlit_tiled(marble, 2.2, 3.0, tile, Color::WHITE));

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(half * 2.0, thick, depth))),
        MeshMaterial3d(floor_mat),
        Transform::from_xyz(0.0, -thick * 0.5, z_mid),
        layer.clone(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(half * 2.0, thick, depth))),
        MeshMaterial3d(ceil_mat),
        Transform::from_xyz(0.0, height + thick * 0.5, z_mid),
        layer.clone(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(thick, height, thick))),
        MeshMaterial3d(wall_front.clone()),
        Transform::from_xyz(0.0, height * 0.5, front - thick * 0.5),
        layer.clone(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(thick, height, thick))),
        MeshMaterial3d(wall_front),
        Transform::from_xyz(0.0, height * 0.5, back + thick * 0.5),
        layer.clone(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(thick, height, depth))),
        MeshMaterial3d(wall_side.clone()),
        Transform::from_xyz(-half - thick * 0.5, height * 0.5, z_mid),
        layer.clone(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(thick, height, depth))),
        MeshMaterial3d(wall_side),
        Transform::from_xyz(half + thick * 0.5, height * 0.5, z_mid),
        layer.clone(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(2.2, 0.72, 3.0))),
        MeshMaterial3d(ledge_mat),
        Transform::from_xyz(-half + 1.1, 1.28, -2.4),
        layer.clone(),
    ));
}

pub(crate) fn rgba_to_image(src: appearance::RgbaImage, nearest: bool) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: src.width,
            height: src.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        src.pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = if nearest {
        ImageSampler::nearest()
    } else {
        ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            ..default()
        })
    };
    image
}

/// 单位圆环网格（外半径 1、内半径 `inner`），供自瞄 FOV 指示用；
/// 屏幕上的椭圆缩放由 Transform 承担，mesh 只生成一次。
pub(crate) fn ring_mesh2d(inner: f32, segments: usize) -> Mesh {
    let mut positions = Vec::with_capacity(segments * 2);
    let mut uvs = Vec::with_capacity(segments * 2);
    let mut indices = Vec::with_capacity(segments * 6);
    for i in 0..segments {
        let a = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let (s, c) = a.sin_cos();
        positions.push([c, s, 0.0]);
        positions.push([c * inner, s * inner, 0.0]);
        uvs.push([0.0, 0.0]);
        uvs.push([0.0, 0.0]);
    }
    for i in 0..segments {
        let o = (i * 2) as u32;
        let n = (((i + 1) % segments) * 2) as u32;
        indices.extend_from_slice(&[o, o + 1, n, n, o + 1, n + 1]);
    }
    mesh2d_from_parts(positions, uvs, indices)
}

pub(crate) fn empty_mesh2d() -> Mesh {
    mesh2d_from_parts(
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
        vec![[0.0, 0.0], [0.0, 0.0], [0.0, 0.0]],
        vec![0, 1, 2],
    )
}

pub(crate) fn mesh2d_from_parts(
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
) -> Mesh {
    let normals = vec![[0.0, 0.0, 1.0]; positions.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}
