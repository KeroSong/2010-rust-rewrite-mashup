//! Local Alex Mercer mode: Prototype's locomotion (`alex_motion`) takes over the
//! local player on a Minecraft world, the way Skate does. K toggles it. The
//! soldier body stands in for Alex until his own model is converted.
use alex_motion::{Alex, Collision, Hit, Input, Params};
use bevy::prelude::*;
use frame::{AlexMode, AppScreen};

/// Third-person camera: height of the look-at point above the feet, distance
/// behind it, and the vertical field of view.
const CAMERA_TARGET_HEIGHT: f32 = 64.0;
const CAMERA_DISTANCE: f32 = 170.0;
const CAMERA_FOV: f32 = 65.0;

/// A Minecraft world is 384 blocks (13 824 units) tall around map z 0; below
/// this Alex is in the void (seen when a spawn lands in unloaded chunks).
const VOID_Z: f32 = -20_000.0;
const SKY_Z: f32 = 16_000.0;

/// Back onto the highest block column under `at`, if the blocks are there.
fn rescue_from_void(at: Vec3) -> Option<Vec3> {
    let top = Vec3::new(at.x, at.y, SKY_Z);
    let bottom = Vec3::new(at.x, at.y, VOID_Z);
    let t = sim::voxel::trace(top.to_array(), bottom.to_array(), alex_motion::MINS, alex_motion::MAXS);
    (t.fraction < 1.0 && t.startsolid == 0).then(|| top + (bottom - top) * t.fraction + Vec3::Z * 2.0)
}

#[derive(Resource, Default)]
struct Host {
    alex: Option<Alex>,
    params: Params,
    log_timer: f32,
    anim: AnimDriver,
    clips: ClipDriver,
}

/// Alex's own Prototype clip for each mode, cross-faded on change.
#[derive(Default)]
struct ClipDriver {
    current: Option<(&'static str, bool)>,
    time: f32,
    previous: Option<(&'static str, bool, f32)>,
    fade: f32,
    dash: f32,
    land: f32,
}

const CLIP_FADE: f32 = 0.15;

impl ClipDriver {
    fn choose(&self, alex: &Alex, params: &Params) -> (&'static str, bool) {
        use alex_motion::Mode;
        let speed = alex.velocity.truncate().length() / alex_motion::UNITS_PER_METRE;
        if self.dash > 0.0 {
            return ("alex_loco_aerial_dash", false);
        }
        match alex.mode {
            Mode::Ground if self.land > 0.0 => ("alex_loco_jump_land_soft", false),
            Mode::Ground if alex.charge.is_some() && speed < 0.5 => ("alex_loco_jump_charged_stand", false),
            Mode::Ground if speed < 0.5 => ("alex_loco_sprint_idle", true),
            Mode::Ground if alex.sprint_time > 0.0 && speed > params.run_speed + 1.0 => ("alex_loco_run_sprint_n", true),
            Mode::Ground if speed > 3.0 => ("alex_loco_run_n", true),
            Mode::Ground => ("alex_loco_walk_n", true),
            Mode::Air if alex.velocity.z > 0.0 => ("alex_loco_jump_long_all1_start", false),
            Mode::Air => ("alex_loco_jump_fall_cycle", true),
            Mode::Glide => ("alex_bullet_smash_glide_forward", true),
            Mode::WallRun => ("alex_wall_run_up", true),
            Mode::Climb if alex.velocity.z > 1.0 => ("alex_climb_up", true),
            Mode::Climb => ("alex_climb_idle_cycle", true),
        }
    }

    fn update(
        &mut self,
        alex: &Alex,
        params: &Params,
        events: &[alex_motion::Event],
        dt: f32,
        rig: &crate::alex_model::Rig,
    ) -> Vec<Mat4> {
        use alex_motion::Event;
        for e in events {
            match e {
                Event::AirDash => self.dash = 0.5,
                Event::Landed => self.land = 0.3,
                Event::Jumped | Event::WallJump | Event::WallStart => self.land = 0.0,
                _ => {}
            }
        }
        self.dash = (self.dash - dt).max(0.0);
        self.land = (self.land - dt).max(0.0);
        let wanted = self.choose(alex, params);
        if self.current != Some(wanted) {
            self.previous = self.current.map(|(name, looped)| (name, looped, self.time));
            self.current = Some(wanted);
            self.time = 0.0;
            self.fade = if self.previous.is_some() { CLIP_FADE } else { 0.0 };
        }
        let (name, looped) = wanted;
        let Some(clip) = rig.clip(name) else {
            return Vec::new();
        };
        // Running loops follow the real speed, so the feet do not skate.
        let speed = alex.velocity.truncate().length();
        let rate = if alex.mode == alex_motion::Mode::Ground && looped && clip.root_speed > 10.0 {
            (speed / clip.root_speed).clamp(0.5, 2.5)
        } else {
            1.0
        };
        self.time += dt * rate;
        self.fade = (self.fade - dt).max(0.0);
        let mut layers = Vec::with_capacity(2);
        if let Some((prev_name, prev_looped, prev_time)) = self.previous.as_mut()
            && self.fade > 0.0
            && let Some(prev) = rig.clip(prev_name)
        {
            *prev_time += dt;
            layers.push(crate::alex_model::Layer { clip: prev, time: *prev_time, looped: *prev_looped, weight: self.fade / CLIP_FADE });
        }
        layers.push(crate::alex_model::Layer { clip, time: self.time, looped, weight: 1.0 - self.fade / CLIP_FADE });
        rig.pose(&layers)
    }
}

/// The soldier stand-in's animation, picked through MW2's own player anim
/// script: indices into `anim_iw4::ANIM_MT_NAMES` and `ANIM_ET_NAMES`.
const MT_IDLE: u8 = 1;
const MT_RUN: u8 = 10;
const MT_CLIMBUP: u8 = 18;
const MT_SPRINT: u8 = 20;
const MT_JUMPDIVE_FORWARD: u8 = 43;
const ET_JUMP: u8 = 3;
const ET_LAND: u8 = 5;
/// Seconds a landing plays before the movement animation takes over again.
const LAND_HOLD: f32 = 0.25;

#[derive(Default)]
struct AnimDriver {
    playing: Option<u8>,
    hold: f32,
}

impl AnimDriver {
    /// The movement type each mode reads as; in the air the jump plays on.
    fn wanted(alex: &Alex, params: &Params) -> Option<u8> {
        let speed = alex.velocity.truncate().length() / alex_motion::UNITS_PER_METRE;
        match alex.mode {
            alex_motion::Mode::Ground if speed < 0.5 => Some(MT_IDLE),
            alex_motion::Mode::Ground if alex.sprint_time > 0.0 && speed > params.run_speed + 1.0 => Some(MT_SPRINT),
            alex_motion::Mode::Ground => Some(MT_RUN),
            alex_motion::Mode::WallRun | alex_motion::Mode::Climb => Some(MT_CLIMBUP),
            alex_motion::Mode::Glide => Some(MT_JUMPDIVE_FORWARD),
            alex_motion::Mode::Air => None,
        }
    }

    fn update(
        &mut self,
        alex: &Alex,
        params: &Params,
        events: &[alex_motion::Event],
        dt: f32,
        authority: &mut net::AuthorityWorld,
        client: sim::ClientId,
    ) {
        use alex_motion::Event;
        let mut event = None;
        let mut forced = None;
        for e in events {
            match e {
                Event::Jumped | Event::WallJump | Event::Vault => {
                    event = Some(ET_JUMP);
                    self.hold = 0.0;
                }
                Event::Landed => {
                    event = Some(ET_LAND);
                    self.hold = LAND_HOLD;
                }
                Event::AirDash => forced = Some(MT_JUMPDIVE_FORWARD),
                _ => {}
            }
        }
        self.hold = (self.hold - dt).max(0.0);
        let wanted = forced.or(if self.hold > 0.0 { None } else { Self::wanted(alex, params) });
        if event.is_some() {
            self.playing = None;
        }
        let movetype = wanted.filter(|m| self.playing != Some(*m));
        if event.is_some() || movetype.is_some() {
            diag::debug!(World, "Alex anim: movetype {movetype:?} event {event:?}");
            authority.0.play_external_anim(client, movetype, event);
            if movetype.is_some() {
                self.playing = movetype;
            }
        }
    }
}

/// The block world, through the same sweep the simulation uses.
struct Blocks;

impl Collision for Blocks {
    fn trace(&self, start: [f32; 3], end: [f32; 3], mins: [f32; 3], maxs: [f32; 3]) -> Hit {
        let t = sim::voxel::trace(start, end, mins, maxs);
        Hit {
            fraction: t.fraction,
            normal: t.normal,
            start_solid: t.startsolid != 0 || t.allsolid != 0,
        }
    }
}

pub fn register(app: &mut App) {
    app.init_resource::<AlexMode>().init_resource::<Host>().add_systems(
        Update,
        update
            .after(frame::PresentedPublished)
            .before(crate::sync_camera_from_presented)
            .before(render_scene::GfxSceneAdd)
            .in_set(frame::ClientSet::Present),
    );
}

fn stop(host: &mut Host, mode: &mut AlexMode, authority: &mut net::AuthorityWorld) {
    authority
        .0
        .set_external_motion(sim::ClientId(mode.client), false);
    host.alex = None;
    host.anim = AnimDriver::default();
    host.clips = ClipDriver::default();
    mode.skin.clear();
    mode.active = false;
    mode.camera = None;
    diag::info!(World, "Alex mode stopped");
}

/// The player's bound movement, sprint and jump, plus the left stick.
fn read_input(actions: &net::ClientActionInput, view_yaw: f32) -> Input {
    let kb = &actions.client.kb;
    let axis = |plus: bool, minus: bool| f32::from(u8::from(plus)) - f32::from(u8::from(minus));
    let forward = axis(kb.forward.active, kb.back.active) + actions.pad_move[0];
    let right = axis(kb.moveright.active, kb.moveleft.active) + actions.pad_move[1];
    Input {
        move_axis: [forward.clamp(-1.0, 1.0), right.clamp(-1.0, 1.0)],
        view_yaw,
        sprint: kb.sprint.active || kb.speed.active,
        jump: kb.gostand.active,
    }
}

fn camera(origin: Vec3, viewangles: [f32; 3]) -> Transform {
    let (pitch, yaw) = (viewangles[0].to_radians(), viewangles[1].to_radians());
    let forward = Vec3::new(pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), -pitch.sin());
    let target = origin + Vec3::Z * CAMERA_TARGET_HEIGHT;
    let wanted = target - forward * CAMERA_DISTANCE;
    // Pull the camera in front of any block between it and Alex.
    let t = sim::voxel::trace(target.to_array(), wanted.to_array(), [-6.0; 3], [6.0; 3]);
    let eye = target + (wanted - target) * t.fraction;
    Transform::from_translation(eye).looking_to(forward, Vec3::Z)
}

fn update(
    time: Res<Time>,
    screen: Res<AppScreen>,
    local: Res<net::LocalPresentClient>,
    presented: Res<net::PresentedSnapshot>,
    actions: Option<Res<net::ClientActionInput>>,
    mut authority: Option<ResMut<net::AuthorityWorld>>,
    mut mode: ResMut<AlexMode>,
    mut host: ResMut<Host>,
) {
    let Some(authority) = authority.as_deref_mut() else {
        return;
    };
    let ps = presented.player(local.0);
    let alive = ps.is_some_and(|p| p.pm_type == 0) && *screen == AppScreen::InGame;
    if mode.active && (!alive || !sim::voxel::active()) {
        stop(&mut host, &mut mode, authority);
    }
    if std::mem::take(&mut mode.toggle_requested) && alive {
        if mode.active {
            stop(&mut host, &mut mode, authority);
            return;
        }
        if !sim::voxel::active() {
            mode.status = "Alex mode needs a Minecraft world (map minecraft:overworld)".into();
            diag::warn!(World, "{}", mode.status);
            return;
        }
        if let Some(ps) = ps {
            let origin = Vec3::from_array(ps.origin) + Vec3::Z * 2.0;
            host.alex = Some(Alex::at(origin.to_array(), ps.viewangles[1]));
            authority.0.set_external_motion(local.0, true);
            mode.active = true;
            mode.client = local.0.0;
            mode.status.clear();
            diag::info!(World, "Alex mode started at {origin}");
        }
    }
    if !mode.active {
        return;
    }
    let Host { alex, params, log_timer, anim, clips } = &mut *host;
    let (Some(ps), Some(alex)) = (ps, alex.as_mut()) else {
        return;
    };
    let input = match actions.as_deref() {
        Some(actions) if !mode.input_blocked => read_input(actions, ps.viewangles[1]),
        _ => Input { view_yaw: ps.viewangles[1], ..Input::default() },
    };
    let dt = time.delta_secs().min(0.1);
    if alex.origin.z < VOID_Z {
        let at = Vec3::from_array(alex.origin.to_array());
        if let Some(ground) = rescue_from_void(at) {
            diag::warn!(World, "Alex fell out of the world at {at}; back on the ground at {ground}");
            *alex = Alex::at(ground.to_array(), alex.heading);
        }
    }
    let events = alex.step(&input, dt, params, &Blocks);
    let origin = Vec3::from_array(alex.origin.to_array());
    authority.0.set_origin(local.0, origin.to_array());
    anim.update(alex, params, &events, dt, authority, local.0);
    if let Some(rig) = crate::alex_model::rig() {
        mode.skin = clips.update(alex, params, &events, dt, rig);
    }
    mode.root = Mat4::from_rotation_translation(Quat::from_rotation_z(alex.heading.to_radians()), origin);
    mode.camera = Some((camera(origin, ps.viewangles), CAMERA_FOV));
    let speed = alex.velocity.length() / alex_motion::UNITS_PER_METRE;
    mode.status = format!("{:?} {:.1} m/s", alex.mode, speed);
    for event in events {
        diag::info!(World, "Alex {event:?} at {origin} ({:?})", alex.mode);
    }
    *log_timer += dt;
    if *log_timer > 2.0 {
        *log_timer = 0.0;
        diag::info!(World, "Alex {} at {origin} legs_anim {:#x}", mode.status, ps.legs_anim);
    }
}
