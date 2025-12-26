//! GLB (Binary glTF) parser for extracting mesh data.
//!
//! GLB format specification: https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#glb-file-format-specification

use serde::Deserialize;

/// Parsed GLB mesh data ready for Roblox Studio.
#[derive(Debug, Clone)]
pub struct GlbMesh {
    /// Vertex positions
    pub vertices: Vec<Vertex>,
    /// Triangle faces (indices into vertices)
    pub faces: Vec<Face>,
    /// Vertex normals
    pub normals: Vec<Normal>,
    /// Texture coordinates
    pub tex_coords: Vec<TexCoord>,
}

/// A vertex position in 3D space.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Vertex {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// A normal vector.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Normal {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// A texture coordinate (UV).
#[derive(Debug, Clone, serde::Serialize)]
pub struct TexCoord {
    pub u: f32,
    pub v: f32,
}

/// A triangle face with vertex indices.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Face {
    /// Indices into the vertices array
    pub vertices: [usize; 3],
    /// Indices into the normals array (if available)
    pub normals: Option<[usize; 3]>,
    /// Indices into the tex_coords array (if available)
    pub tex_coords: Option<[usize; 3]>,
}

/// GLB parsing errors.
#[derive(Debug)]
pub enum GlbParseError {
    /// Invalid GLB header
    InvalidHeader(String),
    /// Unsupported glTF version
    UnsupportedVersion(u32),
    /// Invalid chunk in GLB file
    InvalidChunk(String),
    /// JSON parsing error
    JsonError(String),
    /// No mesh found in GLB
    NoMeshFound,
    /// Unsupported index component type
    UnsupportedIndexType(u32),
    /// Buffer access out of bounds
    BufferOverflow(String),
}

impl std::fmt::Display for GlbParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHeader(s) => write!(f, "Invalid GLB header: {}", s),
            Self::UnsupportedVersion(v) => write!(f, "Unsupported glTF version: {}", v),
            Self::InvalidChunk(s) => write!(f, "Invalid chunk: {}", s),
            Self::JsonError(s) => write!(f, "JSON parse error: {}", s),
            Self::NoMeshFound => write!(f, "No mesh found in GLB"),
            Self::UnsupportedIndexType(t) => write!(f, "Unsupported index type: {}", t),
            Self::BufferOverflow(s) => write!(f, "Buffer overflow: {}", s),
        }
    }
}

impl std::error::Error for GlbParseError {}

// GLB magic number: "glTF" in little-endian
const GLB_MAGIC: u32 = 0x46546C67;
// JSON chunk type: "JSON" in little-endian
const CHUNK_TYPE_JSON: u32 = 0x4E4F534A;
// BIN chunk type: "BIN\0" in little-endian
const CHUNK_TYPE_BIN: u32 = 0x004E4942;

impl GlbMesh {
    /// Parse GLB binary data into mesh components.
    pub fn from_bytes(data: &[u8]) -> Result<Self, GlbParseError> {
        // GLB format:
        // - 12-byte header: magic (4) + version (4) + length (4)
        // - Chunks: JSON chunk + BIN chunk

        if data.len() < 12 {
            return Err(GlbParseError::InvalidHeader("File too small".to_string()));
        }

        // Check magic number
        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if magic != GLB_MAGIC {
            return Err(GlbParseError::InvalidHeader(format!(
                "Invalid magic number: 0x{:08X}",
                magic
            )));
        }

        // Parse version
        let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if version != 2 {
            return Err(GlbParseError::UnsupportedVersion(version));
        }

        // Parse JSON chunk (starts at offset 12)
        if data.len() < 20 {
            return Err(GlbParseError::InvalidChunk(
                "File too small for JSON chunk header".to_string(),
            ));
        }

        let json_chunk_length =
            u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
        let json_chunk_type = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);

        if json_chunk_type != CHUNK_TYPE_JSON {
            return Err(GlbParseError::InvalidChunk(format!(
                "Expected JSON chunk, got 0x{:08X}",
                json_chunk_type
            )));
        }

        if data.len() < 20 + json_chunk_length {
            return Err(GlbParseError::InvalidChunk(
                "JSON chunk extends beyond file".to_string(),
            ));
        }

        let json_data = &data[20..20 + json_chunk_length];
        let gltf: GltfJson =
            serde_json::from_slice(json_data).map_err(|e| GlbParseError::JsonError(e.to_string()))?;

        // Parse BIN chunk (follows JSON chunk, aligned to 4 bytes)
        let bin_offset = 20 + json_chunk_length;
        let bin_offset = (bin_offset + 3) & !3; // Align to 4 bytes

        if data.len() < bin_offset + 8 {
            return Err(GlbParseError::InvalidChunk(
                "Missing BIN chunk".to_string(),
            ));
        }

        let bin_chunk_length = u32::from_le_bytes([
            data[bin_offset],
            data[bin_offset + 1],
            data[bin_offset + 2],
            data[bin_offset + 3],
        ]) as usize;

        let bin_chunk_type = u32::from_le_bytes([
            data[bin_offset + 4],
            data[bin_offset + 5],
            data[bin_offset + 6],
            data[bin_offset + 7],
        ]);

        if bin_chunk_type != CHUNK_TYPE_BIN {
            return Err(GlbParseError::InvalidChunk(format!(
                "Expected BIN chunk, got 0x{:08X}",
                bin_chunk_type
            )));
        }

        if data.len() < bin_offset + 8 + bin_chunk_length {
            return Err(GlbParseError::InvalidChunk(
                "BIN chunk extends beyond file".to_string(),
            ));
        }

        let bin_data = &data[bin_offset + 8..bin_offset + 8 + bin_chunk_length];

        // Extract mesh data from first mesh/primitive
        let mesh = gltf.meshes.first().ok_or(GlbParseError::NoMeshFound)?;
        let primitive = mesh.primitives.first().ok_or(GlbParseError::NoMeshFound)?;

        // Extract vertices (POSITION attribute)
        let vertices = Self::extract_vec3(&gltf, bin_data, primitive.attributes.position)?;

        // Extract normals (NORMAL attribute)
        let normals = if let Some(normal_idx) = primitive.attributes.normal {
            Self::extract_vec3(&gltf, bin_data, normal_idx)?
        } else {
            vec![]
        };

        // Extract texture coordinates (TEXCOORD_0)
        let tex_coords = if let Some(texcoord_idx) = primitive.attributes.texcoord_0 {
            Self::extract_vec2(&gltf, bin_data, texcoord_idx)?
        } else {
            vec![]
        };

        // Extract indices
        let faces = if let Some(indices_idx) = primitive.indices {
            Self::extract_faces(&gltf, bin_data, indices_idx, !normals.is_empty(), !tex_coords.is_empty())?
        } else {
            // No indices - create sequential faces
            let num_triangles = vertices.len() / 3;
            (0..num_triangles)
                .map(|i| Face {
                    vertices: [i * 3, i * 3 + 1, i * 3 + 2],
                    normals: if !normals.is_empty() {
                        Some([i * 3, i * 3 + 1, i * 3 + 2])
                    } else {
                        None
                    },
                    tex_coords: if !tex_coords.is_empty() {
                        Some([i * 3, i * 3 + 1, i * 3 + 2])
                    } else {
                        None
                    },
                })
                .collect()
        };

        Ok(GlbMesh {
            vertices: vertices
                .into_iter()
                .map(|(x, y, z)| Vertex { x, y, z })
                .collect(),
            faces,
            normals: normals
                .into_iter()
                .map(|(x, y, z)| Normal { x, y, z })
                .collect(),
            tex_coords: tex_coords
                .into_iter()
                .map(|(u, v)| TexCoord { u, v })
                .collect(),
        })
    }

    /// Extract vec3 data from an accessor.
    fn extract_vec3(
        gltf: &GltfJson,
        bin: &[u8],
        accessor_idx: usize,
    ) -> Result<Vec<(f32, f32, f32)>, GlbParseError> {
        let accessor = gltf.accessors.get(accessor_idx).ok_or_else(|| {
            GlbParseError::BufferOverflow(format!("Accessor {} not found", accessor_idx))
        })?;

        let buffer_view = gltf.buffer_views.get(accessor.buffer_view).ok_or_else(|| {
            GlbParseError::BufferOverflow(format!(
                "BufferView {} not found",
                accessor.buffer_view
            ))
        })?;

        let offset = buffer_view.byte_offset + accessor.byte_offset.unwrap_or(0);
        let stride = buffer_view.byte_stride.unwrap_or(12); // 3 * sizeof(f32)

        let mut result = Vec::with_capacity(accessor.count);
        for i in 0..accessor.count {
            let base = offset + i * stride;
            if base + 12 > bin.len() {
                return Err(GlbParseError::BufferOverflow(format!(
                    "Vec3 read at {} exceeds buffer size {}",
                    base + 12,
                    bin.len()
                )));
            }
            let x = f32::from_le_bytes([bin[base], bin[base + 1], bin[base + 2], bin[base + 3]]);
            let y = f32::from_le_bytes([
                bin[base + 4],
                bin[base + 5],
                bin[base + 6],
                bin[base + 7],
            ]);
            let z = f32::from_le_bytes([
                bin[base + 8],
                bin[base + 9],
                bin[base + 10],
                bin[base + 11],
            ]);
            result.push((x, y, z));
        }

        Ok(result)
    }

    /// Extract vec2 data from an accessor.
    fn extract_vec2(
        gltf: &GltfJson,
        bin: &[u8],
        accessor_idx: usize,
    ) -> Result<Vec<(f32, f32)>, GlbParseError> {
        let accessor = gltf.accessors.get(accessor_idx).ok_or_else(|| {
            GlbParseError::BufferOverflow(format!("Accessor {} not found", accessor_idx))
        })?;

        let buffer_view = gltf.buffer_views.get(accessor.buffer_view).ok_or_else(|| {
            GlbParseError::BufferOverflow(format!(
                "BufferView {} not found",
                accessor.buffer_view
            ))
        })?;

        let offset = buffer_view.byte_offset + accessor.byte_offset.unwrap_or(0);
        let stride = buffer_view.byte_stride.unwrap_or(8); // 2 * sizeof(f32)

        let mut result = Vec::with_capacity(accessor.count);
        for i in 0..accessor.count {
            let base = offset + i * stride;
            if base + 8 > bin.len() {
                return Err(GlbParseError::BufferOverflow(format!(
                    "Vec2 read at {} exceeds buffer size {}",
                    base + 8,
                    bin.len()
                )));
            }
            let u = f32::from_le_bytes([bin[base], bin[base + 1], bin[base + 2], bin[base + 3]]);
            let v = f32::from_le_bytes([
                bin[base + 4],
                bin[base + 5],
                bin[base + 6],
                bin[base + 7],
            ]);
            result.push((u, v));
        }

        Ok(result)
    }

    /// Extract face indices from an accessor.
    fn extract_faces(
        gltf: &GltfJson,
        bin: &[u8],
        accessor_idx: usize,
        has_normals: bool,
        has_texcoords: bool,
    ) -> Result<Vec<Face>, GlbParseError> {
        let accessor = gltf.accessors.get(accessor_idx).ok_or_else(|| {
            GlbParseError::BufferOverflow(format!("Accessor {} not found", accessor_idx))
        })?;

        let buffer_view = gltf.buffer_views.get(accessor.buffer_view).ok_or_else(|| {
            GlbParseError::BufferOverflow(format!(
                "BufferView {} not found",
                accessor.buffer_view
            ))
        })?;

        let offset = buffer_view.byte_offset + accessor.byte_offset.unwrap_or(0);

        // Determine index type (5123 = UNSIGNED_SHORT, 5125 = UNSIGNED_INT, 5121 = UNSIGNED_BYTE)
        let indices: Vec<usize> = match accessor.component_type {
            5121 => {
                // UNSIGNED_BYTE
                (0..accessor.count)
                    .map(|i| {
                        let base = offset + i;
                        if base >= bin.len() {
                            return Err(GlbParseError::BufferOverflow(format!(
                                "Index read at {} exceeds buffer",
                                base
                            )));
                        }
                        Ok(bin[base] as usize)
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
            5123 => {
                // UNSIGNED_SHORT
                (0..accessor.count)
                    .map(|i| {
                        let base = offset + i * 2;
                        if base + 2 > bin.len() {
                            return Err(GlbParseError::BufferOverflow(format!(
                                "Index read at {} exceeds buffer",
                                base
                            )));
                        }
                        Ok(u16::from_le_bytes([bin[base], bin[base + 1]]) as usize)
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
            5125 => {
                // UNSIGNED_INT
                (0..accessor.count)
                    .map(|i| {
                        let base = offset + i * 4;
                        if base + 4 > bin.len() {
                            return Err(GlbParseError::BufferOverflow(format!(
                                "Index read at {} exceeds buffer",
                                base
                            )));
                        }
                        Ok(u32::from_le_bytes([
                            bin[base],
                            bin[base + 1],
                            bin[base + 2],
                            bin[base + 3],
                        ]) as usize)
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
            _ => return Err(GlbParseError::UnsupportedIndexType(accessor.component_type)),
        };

        // Group into triangles
        let mut faces = Vec::with_capacity(indices.len() / 3);
        for chunk in indices.chunks(3) {
            if chunk.len() == 3 {
                faces.push(Face {
                    vertices: [chunk[0], chunk[1], chunk[2]],
                    normals: if has_normals {
                        Some([chunk[0], chunk[1], chunk[2]])
                    } else {
                        None
                    },
                    tex_coords: if has_texcoords {
                        Some([chunk[0], chunk[1], chunk[2]])
                    } else {
                        None
                    },
                });
            }
        }

        Ok(faces)
    }

    /// Get the number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Get the number of faces.
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }
}

// glTF JSON structures (minimal subset needed for mesh extraction)
#[derive(Debug, Deserialize)]
struct GltfJson {
    accessors: Vec<Accessor>,
    #[serde(rename = "bufferViews")]
    buffer_views: Vec<BufferView>,
    meshes: Vec<Mesh>,
}

#[derive(Debug, Deserialize)]
struct Accessor {
    #[serde(rename = "bufferView")]
    buffer_view: usize,
    #[serde(rename = "byteOffset")]
    byte_offset: Option<usize>,
    #[serde(rename = "componentType")]
    component_type: u32,
    count: usize,
}

#[derive(Debug, Deserialize)]
struct BufferView {
    #[serde(rename = "byteOffset", default)]
    byte_offset: usize,
    #[serde(rename = "byteStride")]
    byte_stride: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct Mesh {
    primitives: Vec<Primitive>,
}

#[derive(Debug, Deserialize)]
struct Primitive {
    attributes: Attributes,
    indices: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct Attributes {
    #[serde(rename = "POSITION")]
    position: usize,
    #[serde(rename = "NORMAL")]
    normal: Option<usize>,
    #[serde(rename = "TEXCOORD_0")]
    texcoord_0: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal valid GLB with a single triangle
    fn create_test_glb() -> Vec<u8> {
        // This is a minimal GLB with:
        // - 3 vertices forming a triangle
        // - 3 indices
        // - No normals or UVs

        let json = r#"{
            "asset": {"version": "2.0"},
            "buffers": [{"byteLength": 48}],
            "bufferViews": [
                {"buffer": 0, "byteOffset": 0, "byteLength": 36},
                {"buffer": 0, "byteOffset": 36, "byteLength": 12}
            ],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"},
                {"bufferView": 1, "componentType": 5125, "count": 3, "type": "SCALAR"}
            ],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}]
        }"#;

        let json_bytes = json.as_bytes();
        let json_len = json_bytes.len();
        // Pad to 4-byte alignment
        let json_padded_len = (json_len + 3) & !3;

        // Binary data: 3 vertices (3 * 3 * 4 = 36 bytes) + 3 indices (3 * 4 = 12 bytes)
        let mut bin_data = Vec::new();
        // Vertex 0: (0, 0, 0)
        bin_data.extend_from_slice(&0.0_f32.to_le_bytes());
        bin_data.extend_from_slice(&0.0_f32.to_le_bytes());
        bin_data.extend_from_slice(&0.0_f32.to_le_bytes());
        // Vertex 1: (1, 0, 0)
        bin_data.extend_from_slice(&1.0_f32.to_le_bytes());
        bin_data.extend_from_slice(&0.0_f32.to_le_bytes());
        bin_data.extend_from_slice(&0.0_f32.to_le_bytes());
        // Vertex 2: (0, 1, 0)
        bin_data.extend_from_slice(&0.0_f32.to_le_bytes());
        bin_data.extend_from_slice(&1.0_f32.to_le_bytes());
        bin_data.extend_from_slice(&0.0_f32.to_le_bytes());
        // Indices: 0, 1, 2
        bin_data.extend_from_slice(&0_u32.to_le_bytes());
        bin_data.extend_from_slice(&1_u32.to_le_bytes());
        bin_data.extend_from_slice(&2_u32.to_le_bytes());

        let bin_len = bin_data.len();

        // Calculate total length
        let total_len = 12 + 8 + json_padded_len + 8 + bin_len;

        let mut glb = Vec::with_capacity(total_len);

        // Header
        glb.extend_from_slice(&GLB_MAGIC.to_le_bytes());
        glb.extend_from_slice(&2_u32.to_le_bytes()); // version
        glb.extend_from_slice(&(total_len as u32).to_le_bytes());

        // JSON chunk
        glb.extend_from_slice(&(json_padded_len as u32).to_le_bytes());
        glb.extend_from_slice(&CHUNK_TYPE_JSON.to_le_bytes());
        glb.extend_from_slice(json_bytes);
        // Pad with spaces
        for _ in 0..(json_padded_len - json_len) {
            glb.push(b' ');
        }

        // BIN chunk
        glb.extend_from_slice(&(bin_len as u32).to_le_bytes());
        glb.extend_from_slice(&CHUNK_TYPE_BIN.to_le_bytes());
        glb.extend_from_slice(&bin_data);

        glb
    }

    #[test]
    fn test_parse_valid_glb() {
        let glb = create_test_glb();
        let mesh = GlbMesh::from_bytes(&glb).unwrap();

        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.face_count(), 1);

        // Check vertices
        assert!((mesh.vertices[0].x - 0.0).abs() < 0.001);
        assert!((mesh.vertices[1].x - 1.0).abs() < 0.001);
        assert!((mesh.vertices[2].y - 1.0).abs() < 0.001);

        // Check face indices
        assert_eq!(mesh.faces[0].vertices, [0, 1, 2]);
    }

    #[test]
    fn test_invalid_magic() {
        let mut data = vec![0u8; 20];
        data[0..4].copy_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        let result = GlbMesh::from_bytes(&data);
        assert!(matches!(result, Err(GlbParseError::InvalidHeader(_))));
    }

    #[test]
    fn test_unsupported_version() {
        let mut data = vec![0u8; 20];
        data[0..4].copy_from_slice(&GLB_MAGIC.to_le_bytes());
        data[4..8].copy_from_slice(&1_u32.to_le_bytes()); // version 1
        let result = GlbMesh::from_bytes(&data);
        assert!(matches!(result, Err(GlbParseError::UnsupportedVersion(1))));
    }

    #[test]
    fn test_file_too_small() {
        let data = vec![0u8; 8];
        let result = GlbMesh::from_bytes(&data);
        assert!(matches!(result, Err(GlbParseError::InvalidHeader(_))));
    }

    #[test]
    fn test_glb_parse_error_display() {
        assert!(GlbParseError::InvalidHeader("test".to_string())
            .to_string()
            .contains("Invalid GLB header"));
        assert!(GlbParseError::UnsupportedVersion(1)
            .to_string()
            .contains("Unsupported glTF version: 1"));
        assert!(GlbParseError::InvalidChunk("test".to_string())
            .to_string()
            .contains("Invalid chunk"));
        assert!(GlbParseError::JsonError("test".to_string())
            .to_string()
            .contains("JSON parse error"));
        assert!(GlbParseError::NoMeshFound
            .to_string()
            .contains("No mesh found"));
        assert!(GlbParseError::UnsupportedIndexType(999)
            .to_string()
            .contains("Unsupported index type: 999"));
        assert!(GlbParseError::BufferOverflow("test".to_string())
            .to_string()
            .contains("Buffer overflow"));
    }

    #[test]
    fn test_vertex_count_empty() {
        let mesh = GlbMesh {
            vertices: vec![],
            faces: vec![],
            normals: vec![],
            tex_coords: vec![],
        };
        assert_eq!(mesh.vertex_count(), 0);
        assert_eq!(mesh.face_count(), 0);
    }
}
