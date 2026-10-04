//! Alex Mercer's own mesh, skeleton and Prototype clips (`IW4L_ALEX_MODEL`,
//! written by `tools/alex_runtime_export.py` from the player's Prototype files).
//! Model space is the soldier's: X forward, Y left, Z up, map units.
use crate::anim::remote_body::{CpuBodyGeom, CpuSurfMeta};
use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct RigJoint {
    name: String,
    parent: i32,
    t: [f32; 3],
    r: [f32; 4],
}

#[derive(Deserialize)]
struct Track {
    joint: usize,
    #[serde(default)]
    rt: Vec<f32>,
    #[serde(default)]
    r: Vec<[f32; 4]>,
    #[serde(default)]
    tt: Vec<f32>,
    #[serde(default)]
    t: Vec<[f32; 3]>,
}

#[derive(Deserialize)]
pub struct Clip {
    pub duration: f32,
    tracks: Vec<Track>,
    /// Forward speed of the clip's root motion, map units per second.
    #[serde(skip)]
    pub root_speed: f32,
}

#[derive(Deserialize)]
pub struct Rig {
    joints: Vec<RigJoint>,
    clips: HashMap<String, Clip>,
    #[serde(skip)]
    inverse_bind: Vec<Mat4>,
    #[serde(skip)]
    root: Option<usize>,
}

pub fn rig() -> Option<&'static Rig> {
    static RIG: OnceLock<Option<Rig>> = OnceLock::new();
    RIG.get_or_init(|| {
        let path = std::env::var_os("IW4L_ALEX_MODEL")?;
        let load = || -> Result<Rig, String> {
            let data = std::fs::read(&path).map_err(|e| e.to_string())?;
            let mut rig: Rig = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
            if rig.joints.iter().enumerate().any(|(i, j)| j.parent >= i as i32) {
                return Err("joints are not ordered parent first".into());
            }
            let rest: Vec<(Vec3, Quat)> = rig.joints.iter().map(|j| (Vec3::from_array(j.t), Quat::from_array(j.r))).collect();
            rig.inverse_bind = world(&rig.joints, &rest).iter().map(Mat4::inverse).collect();
            rig.root = rig.joints.iter().position(|j| j.name == "Motion_Root");
            let root = rig.root;
            for clip in rig.clips.values_mut() {
                clip.root_speed = root
                    .and_then(|r| clip.tracks.iter().find(|t| t.joint == r && t.t.len() > 1))
                    .map(|t| {
                        let (a, b) = (t.t[0], t.t[t.t.len() - 1]);
                        let span = (t.tt[t.tt.len() - 1] - t.tt[0]).max(1e-3);
                        Vec2::new(b[0] - a[0], b[1] - a[1]).length() / span
                    })
                    .unwrap_or(0.0);
            }
            diag::info!(World, "Alex rig: {} joints, {} clips", rig.joints.len(), rig.clips.len());
            Ok(rig)
        };
        load().map_err(|e| diag::warn!(World, "Alex rig: {e}")).ok()
    })
    .as_ref()
}

fn world(joints: &[RigJoint], local: &[(Vec3, Quat)]) -> Vec<Mat4> {
    let mut out: Vec<Mat4> = Vec::with_capacity(joints.len());
    for (j, (t, r)) in joints.iter().zip(local) {
        let m = Mat4::from_rotation_translation(*r, *t);
        out.push(if j.parent >= 0 { out[j.parent as usize] * m } else { m });
    }
    out
}

fn bracket(times: &[f32], t: f32) -> (usize, usize, f32) {
    let k = times.partition_point(|&x| x <= t).saturating_sub(1);
    let k2 = (k + 1).min(times.len() - 1);
    let f = if k2 == k { 0.0 } else { ((t - times[k]) / (times[k2] - times[k])).clamp(0.0, 1.0) };
    (k, k2, f)
}

/// One clip playing: which, where in it, whether it loops, and its weight.
pub struct Layer<'a> {
    pub clip: &'a Clip,
    pub time: f32,
    pub looped: bool,
    pub weight: f32,
}

impl Rig {
    pub fn clip(&self, name: &str) -> Option<&Clip> {
        self.clips.get(name)
    }

    fn sample(&self, layer: &Layer) -> Vec<(Vec3, Quat)> {
        let clip = layer.clip;
        let time = if layer.looped {
            layer.time.rem_euclid(clip.duration.max(1e-3))
        } else {
            layer.time.clamp(0.0, clip.duration)
        };
        let mut local: Vec<(Vec3, Quat)> =
            self.joints.iter().map(|j| (Vec3::from_array(j.t), Quat::from_array(j.r))).collect();
        for track in &clip.tracks {
            let Some(slot) = local.get_mut(track.joint) else {
                continue;
            };
            if !track.rt.is_empty() {
                let (a, b, f) = bracket(&track.rt, time);
                slot.1 = Quat::from_array(track.r[a]).slerp(Quat::from_array(track.r[b]), f).normalize();
            }
            if !track.tt.is_empty() {
                let (a, b, f) = bracket(&track.tt, time);
                slot.0 = Vec3::from_array(track.t[a]).lerp(Vec3::from_array(track.t[b]), f);
            }
        }
        // Alex mode moves him; the clip's own forward and sideways travel is dropped.
        if let Some(root) = self.root {
            let rest = self.joints[root].t;
            local[root].0.x = rest[0];
            local[root].0.y = rest[1];
        }
        local
    }

    /// Skin matrices for a blend of clips (weights need not sum to one).
    pub fn pose(&self, layers: &[Layer]) -> Vec<Mat4> {
        let mut blended: Option<Vec<(Vec3, Quat)>> = None;
        let mut acc = 0.0;
        for layer in layers.iter().filter(|l| l.weight > 0.0) {
            let s = self.sample(layer);
            acc += layer.weight;
            let f = layer.weight / acc;
            blended = Some(match blended {
                None => s,
                Some(b) => b.into_iter().zip(s).map(|((t0, r0), (t1, r1))| (t0.lerp(t1, f), r0.slerp(r1, f))).collect(),
            });
        }
        let local = blended.unwrap_or_else(|| {
            self.joints.iter().map(|j| (Vec3::from_array(j.t), Quat::from_array(j.r))).collect()
        });
        world(&self.joints, &local)
            .into_iter()
            .zip(&self.inverse_bind)
            .map(|(w, ib)| w * *ib)
            .collect()
    }
}

/// Alex's surfaces, skinned with `skin`, in place of the soldier's body and head.
pub(crate) fn skin(skin: &[Mat4], geom: &mut CpuBodyGeom) -> Result<(), String> {
    let model = assets::bot_model::local_alex_model().ok_or("Alex model not loaded")?;
    for surface in &model.surfaces {
        let vertex_base = geom.packed.len() as u32;
        let index_start = geom.indices.len() as u32;
        for v in &surface.vertices {
            let mut position = Vec3::ZERO;
            let mut normal = Vec3::ZERO;
            for i in 0..4 {
                if v.weights[i] <= 0.0 {
                    continue;
                }
                let m = skin.get(v.joints[i]).ok_or("Alex skin matrix missing")?;
                position += m.transform_point3(Vec3::from_array(v.position)) * v.weights[i];
                normal += m.transform_vector3(Vec3::from_array(v.normal)) * v.weights[i];
            }
            normal = normal.normalize_or(Vec3::Z);
            let tangent = normal.any_orthonormal_vector();
            let mut packed = [0u8; asset_iw4::size::GFX_PACKED_VERTEX];
            for (i, f) in position.to_array().iter().enumerate() {
                packed[i * 4..i * 4 + 4].copy_from_slice(&f.to_le_bytes());
            }
            packed[12..16].copy_from_slice(&1.0f32.to_le_bytes());
            packed[16..20].fill(255);
            packed[20..24].copy_from_slice(&v.uv.to_le_bytes());
            packed[24..28].copy_from_slice(&pack_normal(normal));
            packed[28..32].copy_from_slice(&pack_normal(tangent));
            geom.packed.push(packed);
        }
        geom.indices.extend(surface.indices.iter().map(|i| vertex_base + i));
        geom.surfaces.push(CpuSurfMeta {
            index_start,
            index_count: surface.indices.len() as u32,
            name: Some(surface.material.clone()),
        });
    }
    geom.decoded_n = geom.packed.len();
    Ok(())
}

fn pack_normal(n: Vec3) -> [u8; 4] {
    [(n.x * 127.0 + 127.5) as u8, (n.y * 127.0 + 127.5) as u8, (n.z * 127.0 + 127.5) as u8, 63]
}
