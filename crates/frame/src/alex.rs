use bevy::prelude::*;

/// Local Alex Mercer presentation. Locomotion is owned by `alex_motion`, driven
/// from `render_anim::alex`; the soldier body stands in for Alex.
#[derive(Resource, Default)]
pub struct AlexMode {
    pub active: bool,
    pub toggle_requested: bool,
    pub input_blocked: bool,
    pub client: u32,
    pub root: Mat4,
    pub camera: Option<(Transform, f32)>,
    pub status: String,
    /// Alex's own skin matrices (model space: X forward, Z up, map units), from
    /// his Prototype clips; empty while the soldier stands in for him.
    pub skin: Vec<Mat4>,
}
