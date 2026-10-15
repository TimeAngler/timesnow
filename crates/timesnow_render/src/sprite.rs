use std::collections::HashMap;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
}

impl Vertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct TransformUniform {
    rect: [f32; 4],
    params: [f32; 4],
}

const VERTICES: &[Vertex] = &[
    Vertex {
        position: [0.0, 0.0],
        uv: [0.0, 0.0],
    }, // 左上
    Vertex {
        position: [1.0, 0.0],
        uv: [1.0, 0.0],
    }, // 右上
    Vertex {
        position: [1.0, 1.0],
        uv: [1.0, 1.0],
    }, // 右下
    Vertex {
        position: [0.0, 1.0],
        uv: [0.0, 1.0],
    }, // 左下
];

const INDICES: &[u16] = &[0, 1, 2, 0, 2, 3];

/// 每个实例占的字节数，必须对齐到 min_uniform_buffer_offset_alignment
const INSTANCE_ALIGN: u64 = 256;
const MAX_INSTANCES: usize = 1024;

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn fullscreen() -> Self {
        Self::new(0.0, 0.0, 1.0, 1.0)
    }
}

struct TextureEntry {
    /// 这三个字段不直接读取，但必须持有：一旦 drop，wgpu 的纹理/采样器
    /// 就被回收，绑定组会变成悬空引用。
    #[allow(dead_code)]
    texture: wgpu::Texture,
    #[allow(dead_code)]
    view: wgpu::TextureView,
    #[allow(dead_code)]
    sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
}

/// 一个待绘制的 sprite 实例
struct Instance {
    name: String,
    rect: Rect,
    /// 着色。铺一张白纹理 + tint 就等于画纯色块（UI 底色用）。
    tint: [f32; 3],
}

pub struct SpriteRenderer {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    num_indices: u32,
    texture_bgl: wgpu::BindGroupLayout,
    transform_buffer: wgpu::Buffer,
    transform_bind_group: wgpu::BindGroup,

    textures: HashMap<String, TextureEntry>,
    instances: Vec<Instance>,

    /// 本帧已经提示过的缺失 sprite 名（同一帧里一个名字只提示一次）
    reported_missing: std::cell::RefCell<std::collections::HashSet<String>>,
    /// 整个进程内提示过的缺失 sprite 名，避免同一个名字刷满控制台
    warned_missing: std::cell::RefCell<std::collections::HashSet<String>>,

    global_alpha: f32,
    /// 下一批 `queue_tinted` 用的颜色
    next_tint: [f32; 3],
}

impl SpriteRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sprite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sprite.wgsl").into()),
        });

        let texture_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprite texture bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let transform_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprite transform bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<TransformUniform>() as u64,
                    ),
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprite pipeline layout"),
            bind_group_layouts: &[Some(&texture_bgl), Some(&transform_bgl)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sprite pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(Vertex::desc())],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            cache: None,
            multiview_mask: None,
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sprite vb"),
            contents: bytemuck::cast_slice(VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sprite ib"),
            contents: bytemuck::cast_slice(INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        let transform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sprite transforms"),
            size: INSTANCE_ALIGN * MAX_INSTANCES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let transform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sprite transform bg"),
            layout: &transform_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &transform_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<TransformUniform>() as u64),
                }),
            }],
        });

        Self {
            pipeline,
            vertex_buffer,
            index_buffer,
            num_indices: INDICES.len() as u32,
            texture_bgl,
            transform_buffer,
            transform_bind_group,
            textures: HashMap::new(),
            instances: Vec::new(),
            reported_missing: std::cell::RefCell::new(std::collections::HashSet::new()),
            warned_missing: std::cell::RefCell::new(std::collections::HashSet::new()),
            global_alpha: 1.0,
            next_tint: [1.0, 1.0, 1.0],
        }
    }

    pub fn load(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, name: &str, bytes: &[u8]) {
        // 一张坏图不该拖垮整个引擎：跳过它，其它资源照常加载
        let Ok(decoded) = image::load_from_memory(bytes) else {
            eprintln!("纹理解码失败，已跳过: {name}");
            return;
        };
        let img = decoded.to_rgba8();
        let dims = img.dimensions();
        let size = wgpu::Extent3d {
            width: dims.0,
            height: dims.1,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(name),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &img,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * dims.0),
                rows_per_image: Some(dims.1),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sprite texture bg"),
            layout: &self.texture_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        self.textures.insert(
            name.to_string(),
            TextureEntry {
                texture,
                view,
                sampler,
                bind_group,
            },
        );
    }

    pub fn begin_frame(&mut self) {
        self.instances.clear();
        // 每帧重新收集，保证同一个缺失名字在一帧里只提示一次
        self.reported_missing.borrow_mut().clear();
    }

    pub fn queue(&mut self, name: &str, rect: Rect) {
        let tint = self.next_tint;
        self.queue_with(name, rect, tint);
    }

    /// 带着色的绘制。铺一张白纹理 + tint 就等于纯色块（UI 底、遮罩用）。
    pub fn queue_tinted(&mut self, name: &str, rect: Rect, tint: [f32; 3]) {
        self.queue_with(name, rect, tint);
    }

    fn queue_with(&mut self, name: &str, rect: Rect, tint: [f32; 3]) {
        if self.instances.len() >= MAX_INSTANCES {
            eprintln!("超过最大 sprite 数 {MAX_INSTANCES}");
            return;
        }
        self.instances.push(Instance {
            name: name.to_string(),
            rect,
            tint,
        });
    }

    /// 把所有实例的 transform 一次性写入共享 buffer。在 pass 之前调用。
    /// 顺带报告缺失的 sprite：放在这里而不是 `draw` 里，是为了不用 `&mut self`
    /// （`draw` 期间的借用无法可变）。
    pub fn upload(&self, queue: &wgpu::Queue) {
        for (i, inst) in self.instances.iter().enumerate() {
            let transform = instance_transform(inst, self.global_alpha);
            let offset = i as u64 * INSTANCE_ALIGN;
            queue.write_buffer(
                &self.transform_buffer,
                offset,
                bytemuck::bytes_of(&transform),
            );

            // 缺失提示：一个名字整个进程只报一次，不刷屏
            if !self.textures.contains_key(&inst.name)
                && self.reported_missing.borrow_mut().insert(inst.name.clone())
                && self.warned_missing.borrow_mut().insert(inst.name.clone())
            {
                eprintln!("sprite 不存在: {}", inst.name);
            }
        }
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass) {
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

        for (i, inst) in self.instances.iter().enumerate() {
            let Some(entry) = self.textures.get(&inst.name) else {
                continue;
            };
            let offset = (i as u64 * INSTANCE_ALIGN) as u32;
            pass.set_bind_group(0, &entry.bind_group, &[]);
            pass.set_bind_group(1, &self.transform_bind_group, &[offset]);
            pass.draw_indexed(0..self.num_indices, 0, 0..1);
        }
    }

    pub fn set_alpha(&mut self, alpha: f32) {
        self.global_alpha = alpha.clamp(0.0, 1.0);
    }

    /// 设置后续 `queue` 使用的 tint（默认白色 = 不着色）
    pub fn set_tint(&mut self, tint: [f32; 3]) {
        self.next_tint = tint;
    }

    pub fn reset_tint(&mut self) {
        self.next_tint = [1.0, 1.0, 1.0];
    }
}

/// 一个实例 → 上传给 GPU 的 uniform。
///
/// 抽成自由函数是为了能单测：以前这段逻辑埋在 `upload` 里，
/// 需要真的拿到一个 wgpu device 才能验，所以从来没被验过。
fn instance_transform(inst: &Instance, global_alpha: f32) -> TransformUniform {
    TransformUniform {
        rect: [inst.rect.x, inst.rect.y, inst.rect.width, inst.rect.height],
        params: [global_alpha, inst.tint[0], inst.tint[1], inst.tint[2]],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(name: &str, rect: Rect, tint: [f32; 3]) -> Instance {
        Instance {
            name: name.to_string(),
            rect,
            tint,
        }
    }

    #[test]
    fn transform_puts_the_rect_in_xywh_order() {
        let t = instance_transform(&inst("bg", Rect::new(0.1, 0.2, 0.3, 0.4), [1.0; 3]), 1.0);
        assert_eq!(t.rect, [0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn transform_carries_alpha_in_params_x() {
        let i = inst("bg", Rect::fullscreen(), [1.0; 3]);
        assert_eq!(instance_transform(&i, 0.25).params[0], 0.25);
        assert_eq!(instance_transform(&i, 1.0).params[0], 1.0);
        // alpha 为 0 时也要原样传下去（全黑是合法状态）
        assert_eq!(instance_transform(&i, 0.0).params[0], 0.0);
    }

    /// tint 必须落在 params 的 yzw 上——shader 就是按这个约定读的。
    /// 顺序错了会画成奇怪的颜色，而且很难看出来是这里的问题。
    #[test]
    fn transform_carries_tint_in_params_yzw() {
        let i = inst("white", Rect::fullscreen(), [0.1, 0.2, 0.3]);
        let t = instance_transform(&i, 0.5);
        assert_eq!(t.params, [0.5, 0.1, 0.2, 0.3]);
    }

    /// 默认 tint 是白色 = 完全不着色，画面与加 tint 之前一致
    #[test]
    fn default_tint_is_white_so_plain_sprites_are_unchanged() {
        let white = [1.0, 1.0, 1.0];
        let t = instance_transform(&inst("bg", Rect::fullscreen(), white), 1.0);
        assert_eq!(t.params, [1.0, 1.0, 1.0, 1.0]);
    }

    /// uniform 的布局必须和 shader 里的 `Transform` 一致：
    /// 两个 vec4，共 32 字节。改字段时这条会先失败。
    #[test]
    fn uniform_layout_matches_the_shader() {
        assert_eq!(std::mem::size_of::<TransformUniform>(), 32);
        assert_eq!(std::mem::align_of::<TransformUniform>(), 4);
        // 顶点布局：位置 + uv，各两个 f32
        assert_eq!(std::mem::size_of::<Vertex>(), 16);
    }

    /// 实例对齐必须是 256 的倍数（wgpu 的 min_uniform_buffer_offset_alignment 下限），
    /// 否则第 2 个实例开始就会写错位置。
    #[test]
    fn instance_alignment_is_valid() {
        // 与 wgpu::Limits::default() 的 min_uniform_buffer_offset_alignment 一致
        const { assert!(INSTANCE_ALIGN >= 256) };
        assert_eq!(INSTANCE_ALIGN % 256, 0);
        assert!(
            (std::mem::size_of::<TransformUniform>() as u64) <= INSTANCE_ALIGN,
            "一个 uniform 必须放得进一个槽位"
        );
    }

    #[test]
    fn rect_fullscreen_covers_the_whole_screen() {
        let r = Rect::fullscreen();
        assert_eq!((r.x, r.y, r.width, r.height), (0.0, 0.0, 1.0, 1.0));
    }
}
