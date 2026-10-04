//! Alex Mercer's locomotion in map space: Z up, origin at the feet, 36 units per
//! metre. Five modes: ground (run, ramped sprint, jump charged while held),
//! air (two dashes on Run, a hop into the glide on a second Jump), glide, wall
//! run and climb. Collision is whatever implements
//! [`Collision`]; the game hands it the block world, tests hand it boxes.
mod params;

use glam::Vec3;
pub use params::{Params, UNITS_PER_METRE};

/// IW4's standing player box, feet at the origin.
pub const MINS: [f32; 3] = [-15.0, -15.0, 0.0];
pub const MAXS: [f32; 3] = [15.0, 15.0, 70.0];

const GROUND_PROBE: f32 = 2.5;
const CLEARANCE: f32 = 0.1;
const WALL_PROBE: f32 = 8.0;
const WALKABLE: f32 = 0.7;
/// A wall is taken when run into within about 40 degrees of head-on; glancing
/// along it is not running up it (seen in game: a 45 degree graze under a low
/// ceiling flickered between wall and ground).
const WALL_FACING: f32 = 0.77;
const WALL_COOLDOWN: f32 = 0.4;

#[derive(Clone, Copy, Debug, Default)]
pub struct Hit {
    pub fraction: f32,
    pub normal: [f32; 3],
    pub start_solid: bool,
}

pub trait Collision {
    /// Sweep the box `mins..maxs` from `start` to `end`.
    fn trace(&self, start: [f32; 3], end: [f32; 3], mins: [f32; 3], maxs: [f32; 3]) -> Hit;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    /// Forward and right, -1 to 1, relative to the camera.
    pub move_axis: [f32; 2],
    /// Camera yaw in degrees, IW4 convention: 0 is +X, 90 is +Y.
    pub view_yaw: f32,
    pub sprint: bool,
    pub jump: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Ground,
    Air,
    Glide,
    WallRun,
    Climb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Jumped,
    GlideHop,
    AirDash,
    Landed,
    WallStart,
    WallLeap,
    WallJump,
    Vault,
    Unstuck,
}

#[derive(Clone, Debug)]
pub struct Alex {
    /// Feet, map units.
    pub origin: Vec3,
    /// Map units per second.
    pub velocity: Vec3,
    /// Facing, degrees.
    pub heading: f32,
    pub mode: Mode,
    /// Seconds in the current mode.
    pub mode_time: f32,
    /// Seconds of uninterrupted sprint.
    pub sprint_time: f32,
    /// Seconds the jump has been charging.
    pub charge: Option<f32>,
    pub wall_normal: Vec3,
    air_dashes_left: u32,
    /// Hops into the glide left for this time in the air.
    glide_hops_left: u32,
    /// Upward speed of a leap up a wall, map units per second; 0 when not leaping.
    wall_leap: f32,
    /// Seconds before a wall can be taken again after dropping off one.
    wall_cooldown: f32,
    /// Seconds left of the dash's level flight, then of its braking.
    dash_timer: f32,
    brake_timer: f32,
    jump_was_down: bool,
    sprint_was_down: bool,
}

fn m(metres: f32) -> f32 {
    metres * UNITS_PER_METRE
}

fn yaw_of(v: Vec3) -> f32 {
    v.y.atan2(v.x).to_degrees()
}

fn dir_of(yaw: f32) -> Vec3 {
    let r = yaw.to_radians();
    Vec3::new(r.cos(), r.sin(), 0.0)
}

/// Turn `from` toward `to` by at most `step` degrees.
fn approach_angle(from: f32, to: f32, step: f32) -> f32 {
    let delta = (to - from + 540.0).rem_euclid(360.0) - 180.0;
    from + delta.clamp(-step, step)
}

fn walkable(n: Vec3) -> bool {
    n.z > WALKABLE
}

fn launch_speed(gravity: f32, height: f32) -> f32 {
    (2.0 * m(gravity) * m(height)).sqrt()
}

/// Camera-relative wish direction, length 0 to 1.
fn wish(input: &Input) -> Vec3 {
    let forward = dir_of(input.view_yaw);
    let right = Vec3::new(forward.y, -forward.x, 0.0);
    let w = forward * input.move_axis[0] + right * input.move_axis[1];
    if w.length() > 1.0 { w.normalize() } else { w }
}

/// Quake-style slide: move, clip the velocity on what was hit, repeat.
/// Returns the normals of every surface touched.
fn slide(world: &impl Collision, pos: &mut Vec3, vel: &mut Vec3, dt: f32) -> Vec<Vec3> {
    let mut left = dt;
    let mut hits: Vec<Vec3> = Vec::new();
    for _ in 0..4 {
        if left <= 0.0 || vel.length_squared() < 1e-6 {
            break;
        }
        let end = *pos + *vel * left;
        let h = world.trace(pos.to_array(), end.to_array(), MINS, MAXS);
        if h.start_solid {
            break;
        }
        *pos += (end - *pos) * h.fraction;
        if h.fraction >= 1.0 {
            break;
        }
        let n = Vec3::from_array(h.normal);
        *pos += n * CLEARANCE;
        left *= 1.0 - h.fraction;
        *vel -= n * vel.dot(n);
        if let Some(&previous) = hits.last() {
            let crease = previous.cross(n);
            if crease.length_squared() > 1e-6 {
                let d = crease.normalize();
                *vel = d * vel.dot(d);
            }
        }
        hits.push(n);
    }
    hits
}

impl Alex {
    pub fn new(origin: Vec3, heading: f32) -> Self {
        Self {
            origin,
            velocity: Vec3::ZERO,
            heading,
            mode: Mode::Air,
            mode_time: 0.0,
            sprint_time: 0.0,
            charge: None,
            wall_normal: Vec3::X,
            air_dashes_left: 0,
            glide_hops_left: 0,
            wall_leap: 0.0,
            wall_cooldown: 0.0,
            dash_timer: 0.0,
            brake_timer: 0.0,
            jump_was_down: false,
            sprint_was_down: false,
        }
    }

    /// A fresh time in the air: dashes and the glide hop are available again.
    fn take_off(&mut self, p: &Params) {
        self.air_dashes_left = p.air_dashes;
        self.glide_hops_left = p.glide_hops;
        self.wall_leap = 0.0;
        self.dash_timer = 0.0;
        self.brake_timer = 0.0;
    }

    /// Same as [`Alex::new`], for callers on another glam version.
    pub fn at(origin: [f32; 3], heading: f32) -> Self {
        Self::new(Vec3::from_array(origin), heading)
    }

    fn set_mode(&mut self, mode: Mode) {
        if self.mode != mode {
            self.mode = mode;
            self.mode_time = 0.0;
        }
    }

    fn is_wall(&self, n: Vec3, p: &Params) -> bool {
        n.z.abs() <= p.wall_min_angle.to_radians().cos()
    }

    fn ground_below(&self, world: &impl Collision, depth: f32) -> Option<(f32, Vec3)> {
        let end = self.origin - Vec3::Z * depth;
        let h = world.trace(self.origin.to_array(), end.to_array(), MINS, MAXS);
        let n = Vec3::from_array(h.normal);
        (!h.start_solid && h.fraction < 1.0 && walkable(n)).then_some((h.fraction * depth, n))
    }

    /// Out of any block Alex is inside (blocks streamed in around him, or a
    /// rescue that put him in one): up a block at a time, up to ten.
    fn unstick(&mut self, world: &impl Collision) -> bool {
        let at = |z: f32| self.origin + Vec3::Z * z;
        if !world.trace(at(0.0).to_array(), at(0.0).to_array(), MINS, MAXS).start_solid {
            return false;
        }
        for k in 1..=10 {
            let up = at(k as f32 * UNITS_PER_METRE);
            if !world.trace(up.to_array(), up.to_array(), MINS, MAXS).start_solid {
                self.origin = up;
                self.velocity = Vec3::ZERO;
                self.set_mode(Mode::Air);
                return true;
            }
        }
        false
    }

    /// Advance by `dt` seconds.
    pub fn step(&mut self, input: &Input, dt: f32, p: &Params, world: &impl Collision) -> Vec<Event> {
        let dt = dt.clamp(0.0, 0.1);
        let unstuck = self.unstick(world);
        let pressed = input.jump && !self.jump_was_down;
        let released = !input.jump && self.jump_was_down;
        let sprint_pressed = input.sprint && !self.sprint_was_down;
        self.jump_was_down = input.jump;
        self.sprint_was_down = input.sprint;
        self.mode_time += dt;
        self.wall_cooldown = (self.wall_cooldown - dt).max(0.0);
        let mut events = Vec::new();
        if unstuck {
            events.push(Event::Unstuck);
        }
        match self.mode {
            Mode::Ground => self.ground(input, pressed, released, dt, p, world, &mut events),
            Mode::Air | Mode::Glide => self.air(input, pressed, sprint_pressed, dt, p, world, &mut events),
            Mode::WallRun | Mode::Climb => self.wall(input, pressed, released, dt, p, world, &mut events),
        }
        events
    }

    fn jump(&mut self, height: f32, p: &Params, events: &mut Vec<Event>) {
        self.velocity.z = launch_speed(p.gravity, height);
        self.charge = None;
        self.take_off(p);
        self.set_mode(Mode::Air);
        events.push(Event::Jumped);
    }

    #[allow(clippy::too_many_arguments)]
    fn ground(
        &mut self,
        input: &Input,
        pressed: bool,
        released: bool,
        dt: f32,
        p: &Params,
        world: &impl Collision,
        events: &mut Vec<Event>,
    ) {
        if self.ground_below(world, GROUND_PROBE).is_none() {
            self.charge = None;
            self.take_off(p);
            self.set_mode(Mode::Air);
            return;
        }
        let w = wish(input);
        let still = w.length() < 0.2;
        // Jump charges while held, standing or on the move, and leaves on
        // release: a tap is the plain jump, a full second the charged one.
        if let Some(c) = self.charge.as_mut() {
            *c += dt;
        }
        if pressed {
            self.charge = Some(0.0);
        }
        if released && let Some(c) = self.charge {
            let t = (c / p.charge_time).clamp(0.0, 1.0);
            self.jump(p.jump_height + (p.charged_jump_height - p.jump_height) * t, p, events);
            return;
        }
        let sprinting = input.sprint && !still;
        self.sprint_time = if sprinting { self.sprint_time + dt } else { 0.0 };
        let horizontal = self.velocity.with_z(0.0);
        let speed = horizontal.length();
        let mut target = if sprinting {
            let t = (self.sprint_time / p.sprint_ramp_time).clamp(0.0, 1.0);
            p.sprint_start_speed + (p.sprint_max_speed - p.sprint_start_speed) * t
        } else {
            p.run_speed
        };
        if !still {
            let desired = yaw_of(w);
            let fast = ((speed - m(p.run_speed)) / m(p.sprint_max_speed - p.run_speed)).clamp(0.0, 1.0);
            let rate = p.turn_rate_slow + (p.turn_rate_fast - p.turn_rate_slow) * fast;
            let off = ((desired - self.heading + 540.0).rem_euclid(360.0) - 180.0).abs();
            // A hard turn at sprint costs speed.
            if sprinting && off > 90.0 {
                target = target.min(p.sprint_min_speed);
                self.sprint_time = 0.0;
            }
            self.heading = approach_angle(self.heading, desired, rate * dt);
        }
        let dir = if sprinting { dir_of(self.heading) } else { w.normalize_or_zero() };
        let goal = if still { Vec3::ZERO } else { dir * m(target) * w.length().max(0.5) };
        let accel = if goal.length() < horizontal.length() { p.ground_brake } else { p.ground_accel };
        let delta = goal - horizontal;
        let change = delta.clamp_length_max(m(accel) * dt);
        self.velocity = (horizontal + change).with_z(0.0);
        let hits = self.ground_move(dt, p, world);
        for n in hits {
            if self.is_wall(n, p) && dir.dot(-n) > WALL_FACING && sprinting && self.wall_cooldown <= 0.0 {
                self.wall_normal = n;
                self.set_mode(Mode::WallRun);
                events.push(Event::WallStart);
                return;
            }
        }
    }

    /// Ground slide with a one-block step up and a snap down onto the ground.
    fn ground_move(&mut self, dt: f32, p: &Params, world: &impl Collision) -> Vec<Vec3> {
        let start = self.origin;
        let vel = self.velocity;
        let mut a_pos = start;
        let mut a_vel = vel;
        let a_hits = slide(world, &mut a_pos, &mut a_vel, dt);
        let mut result = (a_pos, a_vel, a_hits);
        if result.2.iter().any(|n| !walkable(*n)) {
            let step = m(p.step_height);
            let up = world.trace(start.to_array(), (start + Vec3::Z * step).to_array(), MINS, MAXS);
            if !up.start_solid {
                let mut b_pos = start + Vec3::Z * step * up.fraction;
                let mut b_vel = vel;
                let b_hits = slide(world, &mut b_pos, &mut b_vel, dt);
                let down = world.trace(b_pos.to_array(), (b_pos - Vec3::Z * (step + GROUND_PROBE)).to_array(), MINS, MAXS);
                let landed = !down.start_solid && down.fraction < 1.0 && walkable(Vec3::from_array(down.normal));
                let b_end = b_pos - Vec3::Z * (step + GROUND_PROBE) * down.fraction + Vec3::Z * CLEARANCE;
                let gain = (b_end - start).with_z(0.0).length() - (result.0 - start).with_z(0.0).length();
                if landed && gain > 1.0 {
                    result = (b_end, b_vel, b_hits);
                }
            }
        }
        self.origin = result.0;
        self.velocity = result.1.with_z(0.0);
        let snap = m(p.step_height);
        if let Some((d, _)) = self.ground_below(world, snap) {
            self.origin.z -= (d - CLEARANCE).max(0.0);
        }
        result.2
    }

    #[allow(clippy::too_many_arguments)]
    fn air(
        &mut self,
        input: &Input,
        pressed: bool,
        sprint_pressed: bool,
        dt: f32,
        p: &Params,
        world: &impl Collision,
        events: &mut Vec<Event>,
    ) {
        let w = wish(input);
        // Run in the air dashes, level, toward the stick or else the camera.
        if sprint_pressed && self.air_dashes_left > 0 {
            self.air_dashes_left -= 1;
            let dir = if w.length() > 0.2 { w.normalize() } else { dir_of(input.view_yaw) };
            self.velocity = dir * m(p.air_dash_speed);
            self.heading = yaw_of(dir);
            self.dash_timer = p.air_dash_time;
            self.brake_timer = 0.0;
            self.set_mode(Mode::Air);
            events.push(Event::AirDash);
        }
        // Jump pressed in the air hops (twice per time in the air), then glides while held.
        if pressed && self.dash_timer <= 0.0 {
            if self.glide_hops_left > 0 {
                self.glide_hops_left -= 1;
                // Never slower upward than before: early in a charged jump it would cut the climb.
                self.velocity.z = self.velocity.z.max(launch_speed(p.glide_gravity, p.glide_hop_height));
                events.push(Event::GlideHop);
            }
            self.set_mode(Mode::Glide);
        } else if !input.jump && self.mode == Mode::Glide {
            self.set_mode(Mode::Air);
        }
        if self.dash_timer > 0.0 {
            self.dash_timer -= dt;
            if self.dash_timer <= 0.0 {
                self.brake_timer = p.dash_brake_time;
            }
        } else {
            let gliding = self.mode == Mode::Glide;
            let (gravity, cap) = if gliding { (p.glide_gravity, p.glide_max_sink) } else { (p.gravity, p.max_fall_speed) };
            self.velocity.z = (self.velocity.z - m(gravity) * dt).max(-m(cap));
        }
        let horizontal = self.velocity.with_z(0.0);
        if self.dash_timer > 0.0 {
            // Level flight: the dash velocity stands.
        } else if self.brake_timer > 0.0 {
            self.brake_timer -= dt;
            let speed = horizontal.length();
            let slowed = (speed - m(p.dash_brake_decel) * dt).max(m(p.dash_brake_speed).min(speed));
            self.velocity = (horizontal.normalize_or_zero() * slowed).with_z(self.velocity.z);
        } else if self.mode == Mode::Glide {
            if w.length() > 0.2 {
                self.heading = approach_angle(self.heading, yaw_of(w), p.turn_rate_fast * dt);
            }
            let speed = horizontal.length().max(m(p.glide_speed));
            let h = dir_of(self.heading) * speed;
            self.velocity = h.with_z(self.velocity.z);
        } else if w.length() > 0.1 {
            let top = horizontal.length().max(m(p.run_speed));
            let goal = w * top;
            let change = (goal - horizontal).clamp_length_max(m(p.air_accel) * dt);
            self.velocity = (horizontal + change).with_z(self.velocity.z);
            if horizontal.length() > 1.0 {
                self.heading = approach_angle(self.heading, yaw_of(self.velocity), p.turn_rate_fast * dt);
            }
        }
        let falling = self.velocity.z <= 0.0;
        let mut pos = self.origin;
        let mut vel = self.velocity;
        let hits = slide(world, &mut pos, &mut vel, dt);
        self.origin = pos;
        self.velocity = vel;
        for n in hits {
            if walkable(n) && falling {
                self.velocity.z = 0.0;
                self.set_mode(Mode::Ground);
                events.push(Event::Landed);
                return;
            }
            if self.is_wall(n, p) && w.dot(-n) > WALL_FACING && self.wall_cooldown <= 0.0 {
                self.wall_normal = n;
                self.set_mode(if input.sprint { Mode::WallRun } else { Mode::Climb });
                events.push(Event::WallStart);
                return;
            }
        }
        if falling && self.ground_below(world, GROUND_PROBE).is_some() {
            self.velocity.z = 0.0;
            self.set_mode(Mode::Ground);
            events.push(Event::Landed);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn wall(
        &mut self,
        input: &Input,
        pressed: bool,
        released: bool,
        dt: f32,
        p: &Params,
        world: &impl Collision,
        events: &mut Vec<Event>,
    ) {
        let n = self.wall_normal;
        let w = wish(input);
        let probe = world.trace(self.origin.to_array(), (self.origin - n * WALL_PROBE).to_array(), MINS, MAXS);
        if !probe.start_solid && probe.fraction >= 1.0 {
            // Past the top: vault onto it, keeping a leap's momentum.
            self.velocity = Vec3::Z * self.velocity.z.max(m(4.0)) - n * m(5.0);
            self.wall_leap = 0.0;
            self.set_mode(Mode::Air);
            events.push(Event::Vault);
            return;
        }
        let found = Vec3::from_array(probe.normal);
        if probe.fraction < 1.0 && self.is_wall(found, p) {
            self.wall_normal = found;
        }
        let n = self.wall_normal;
        // Jump charges on the wall too; on release Alex leaps straight up it,
        // or pushes off when the stick points away from the wall.
        if let Some(c) = self.charge.as_mut() {
            *c += dt;
        }
        if pressed {
            self.charge = Some(0.0);
        }
        if released && let Some(c) = self.charge.take() {
            let t = (c / p.charge_time).clamp(0.0, 1.0);
            let height = p.jump_height + (p.charged_jump_height - p.jump_height) * t;
            if w.dot(n) > 0.5 {
                self.velocity = n * m(p.wall_jump_out) + Vec3::Z * launch_speed(p.gravity, height);
                self.heading = yaw_of(n);
                self.wall_leap = 0.0;
                self.take_off(p);
                self.set_mode(Mode::Air);
                events.push(Event::WallJump);
                return;
            }
            self.wall_leap = launch_speed(p.gravity, height);
            events.push(Event::WallLeap);
        }
        let leaping = self.wall_leap > 0.0;
        let leaving = (self.mode == Mode::WallRun && !input.sprint && !leaping && self.charge.is_none())
            || (self.mode == Mode::Climb && w.dot(n) > 0.5);
        if leaving {
            self.velocity = n * m(3.0);
            self.wall_cooldown = WALL_COOLDOWN;
            self.take_off(p);
            self.set_mode(Mode::Air);
            return;
        }
        let speed = if self.mode == Mode::WallRun {
            let t = (self.mode_time / p.wall_run_ramp_time).clamp(0.0, 1.0);
            m(p.wall_run_start_speed + (p.wall_run_max_speed - p.wall_run_start_speed) * t)
        } else if w.length() > 0.2 {
            m(p.climb_speed)
        } else {
            0.0
        };
        // A leap adds to the climb and dies away under gravity: a tap gains the
        // plain jump height on top of the run, a full charge the charged height.
        let rise = speed + self.wall_leap;
        if leaping {
            self.wall_leap = (self.wall_leap - m(p.gravity) * dt).max(0.0);
        }
        let tangent = Vec3::Z.cross(n).normalize_or_zero();
        let lateral = w.dot(tangent);
        self.velocity = Vec3::Z * rise * (1.0 - lateral.abs() * 0.4) + tangent * lateral * speed * 0.8 - n * m(2.0);
        self.heading = yaw_of(-n);
        let mut pos = self.origin;
        let mut vel = self.velocity;
        let hits = slide(world, &mut pos, &mut vel, dt);
        self.origin = pos;
        for h in hits {
            if h.z < -WALKABLE {
                self.velocity = n * m(2.0);
                self.wall_cooldown = WALL_COOLDOWN;
                self.wall_leap = 0.0;
                self.set_mode(Mode::Air);
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests;
