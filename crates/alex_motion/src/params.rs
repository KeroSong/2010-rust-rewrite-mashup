//! Tuning, in metres, seconds and degrees. Values marked `fig0` were read out of
//! Prototype's own motion trees (`startup_fig`: prototype_sprint / _jump / _air /
//! _wall); the field each one came from is named. The meaning of a field is
//! inferred by comparing every instance in the game (see NOTES.md, "Sens des
//! champs"), so those are strong guesses, not certainties. Values marked
//! `guess` have no source yet and are tuned by feel.

/// One Minecraft block is 36 map units and is taken as one metre.
pub const UNITS_PER_METRE: f32 = 36.0;

#[derive(Clone, Debug)]
pub struct Params {
    /// Running without the sprint button. guess
    pub run_speed: f32,
    /// locoSprint c03 / c04 / c05 (fig0): speed while turning hard, at sprint
    /// start, and at full sprint.
    pub sprint_min_speed: f32,
    pub sprint_start_speed: f32,
    pub sprint_max_speed: f32,
    /// locoSprint c06 (fig0): seconds from sprint start to full sprint.
    pub sprint_ramp_time: f32,
    /// locoSprint c10 / c11 (fig0): heading turn rate at low and full speed, deg/s.
    pub turn_rate_slow: f32,
    pub turn_rate_fast: f32,
    /// Ground acceleration and braking, m/s². guess
    pub ground_accel: f32,
    pub ground_brake: f32,
    /// fall c13 (fig0): most of Alex's fall nodes use 25 to 50 m/s².
    pub gravity: f32,
    /// fall c14 (fig0): terminal fall speed, 50 to 70 m/s.
    pub max_fall_speed: f32,
    /// jump c05 / c06 of prototype_jump "release" (fig0): apex height range,
    /// tapped jump and full charge. Jump charges while held, standing or
    /// moving, and leaves on release.
    pub jump_height: f32,
    pub charged_jump_height: f32,
    /// super_jump condition `charge(1, Jump)` (fig0): seconds of charge for full height.
    pub charge_time: f32,
    /// prototype_air jump c05 / c06 = 0.5 (fig0): the hop before a glide, on a
    /// Jump press in the air; two per jump (Henri, from the game).
    pub glide_hop_height: f32,
    pub glide_hops: u32,
    /// prototype_air `bank:dash` (fig0): Run pressed in the air, counted by
    /// `air_dash`; two with the upgrade.
    pub air_dashes: u32,
    /// prototype_air dash track c03..c04 = 0 .. 0.2 s, c05 = 40 (fig0): dash speed, m/s,
    /// and how long it holds Alex level.
    pub air_dash_speed: f32,
    pub air_dash_time: f32,
    /// prototype_air flyingBrake c04 = 15, c06 = 50, 0.2 .. 0.67 s (fig0): speed the
    /// dash brakes back to, its deceleration, and for how long.
    pub dash_brake_speed: f32,
    pub dash_brake_decel: f32,
    pub dash_brake_time: f32,
    /// Steering in the air, m/s². guess
    pub air_accel: f32,
    /// prototype_air fall nodes with c13 = 9.8 (fig0) are taken as the glide.
    pub glide_gravity: f32,
    /// prototype_air `velocity(Locomotion, Y, <, -5)` (fig0): glide sink cap, m/s.
    pub glide_max_sink: f32,
    /// Forward speed kept while gliding. guess
    pub glide_speed: f32,
    /// wallRun c03 / c04 / c05 (fig0): climb speed at start and at full run, and
    /// the seconds between them.
    pub wall_run_start_speed: f32,
    pub wall_run_max_speed: f32,
    pub wall_run_ramp_time: f32,
    /// wallRun c07 (fig0): a surface steeper than this (deg from the floor) is a wall.
    pub wall_min_angle: f32,
    /// Climbing without the sprint button. guess (locoClimb c03 is 25, unit unknown)
    pub climb_speed: f32,
    /// On a wall, Jump (charged like on the ground) leaps straight up it; with
    /// the stick pulled away it pushes off at this speed, m/s. guess (wallJump
    /// c03..c06 hold 25 / 35 pairs whose unit is unknown)
    pub wall_jump_out: f32,
    /// Highest step taken without jumping: one block. guess
    pub step_height: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            run_speed: 8.0,
            sprint_min_speed: 10.0,
            sprint_start_speed: 14.0,
            sprint_max_speed: 20.0,
            sprint_ramp_time: 2.5,
            turn_rate_slow: 360.0,
            turn_rate_fast: 90.0,
            ground_accel: 40.0,
            ground_brake: 60.0,
            gravity: 30.0,
            max_fall_speed: 60.0,
            jump_height: 5.0,
            charged_jump_height: 25.0,
            charge_time: 1.0,
            glide_hop_height: 0.5,
            glide_hops: 2,
            air_dashes: 2,
            air_dash_speed: 40.0,
            air_dash_time: 0.2,
            dash_brake_speed: 15.0,
            dash_brake_decel: 50.0,
            dash_brake_time: 0.47,
            air_accel: 15.0,
            glide_gravity: 9.8,
            glide_max_sink: 5.0,
            glide_speed: 14.0,
            wall_run_start_speed: 10.0,
            wall_run_max_speed: 16.0,
            wall_run_ramp_time: 3.0,
            wall_min_angle: 80.0,
            climb_speed: 5.0,
            wall_jump_out: 10.0,
            step_height: 1.1,
        }
    }
}
