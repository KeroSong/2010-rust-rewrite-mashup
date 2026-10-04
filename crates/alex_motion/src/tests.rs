use super::*;

/// Axis-aligned boxes, swept with the player box by Minkowski slabs.
struct Boxes(Vec<(Vec3, Vec3)>);

impl Collision for Boxes {
    fn trace(&self, start: [f32; 3], end: [f32; 3], mins: [f32; 3], maxs: [f32; 3]) -> Hit {
        let s = Vec3::from_array(start);
        let d = Vec3::from_array(end) - s;
        let mut best = Hit { fraction: 1.0, ..Hit::default() };
        for (lo, hi) in &self.0 {
            let lo = *lo - Vec3::from_array(maxs);
            let hi = *hi - Vec3::from_array(mins);
            let inside = (0..3).all(|a| s[a] > lo[a] + 0.01 && s[a] < hi[a] - 0.01);
            if inside {
                return Hit { fraction: 0.0, normal: [0.0; 3], start_solid: true };
            }
            let (mut t_in, mut t_out, mut normal) = (f32::NEG_INFINITY, f32::INFINITY, Vec3::ZERO);
            let mut miss = false;
            for a in 0..3 {
                if d[a].abs() < 1e-9 {
                    if s[a] <= lo[a] || s[a] >= hi[a] {
                        miss = true;
                    }
                    continue;
                }
                let (mut t1, mut t2) = ((lo[a] - s[a]) / d[a], (hi[a] - s[a]) / d[a]);
                let mut n = Vec3::ZERO;
                n[a] = -1.0;
                if t1 > t2 {
                    std::mem::swap(&mut t1, &mut t2);
                    n[a] = 1.0;
                }
                if t1 > t_in {
                    t_in = t1;
                    normal = n;
                }
                t_out = t_out.min(t2);
            }
            if !miss && t_in <= t_out && t_in >= -0.001 && t_in < best.fraction {
                best = Hit { fraction: t_in.max(0.0), normal: normal.to_array(), start_solid: false };
            }
        }
        best
    }
}

const DT: f32 = 1.0 / 60.0;

fn floor() -> (Vec3, Vec3) {
    (Vec3::new(-10000.0, -10000.0, -100.0), Vec3::new(10000.0, 10000.0, 0.0))
}

fn run(alex: &mut Alex, input: Input, seconds: f32, world: &Boxes) -> Vec<Event> {
    let p = Params::default();
    let mut all = Vec::new();
    for _ in 0..(seconds / DT).round() as usize {
        all.extend(alex.step(&input, DT, &p, world));
    }
    all
}

fn standing(world: &Boxes) -> Alex {
    let mut alex = Alex::new(Vec3::new(0.0, 0.0, 0.2), 0.0);
    run(&mut alex, Input::default(), 0.2, world);
    assert_eq!(alex.mode, Mode::Ground);
    alex
}

fn forward(sprint: bool) -> Input {
    Input { move_axis: [1.0, 0.0], view_yaw: 0.0, sprint, jump: false }
}

fn speed_mps(alex: &Alex) -> f32 {
    alex.velocity.with_z(0.0).length() / UNITS_PER_METRE
}

#[test]
fn settles_on_the_floor_and_stays() {
    let world = Boxes(vec![floor()]);
    let mut alex = standing(&world);
    run(&mut alex, Input::default(), 1.0, &world);
    assert!(alex.origin.z.abs() < 1.0, "z = {}", alex.origin.z);
    assert!(alex.origin.with_z(0.0).length() < 0.5);
}

#[test]
fn runs_at_run_speed() {
    let world = Boxes(vec![floor()]);
    let mut alex = standing(&world);
    run(&mut alex, forward(false), 1.0, &world);
    assert!((speed_mps(&alex) - Params::default().run_speed).abs() < 0.2, "{}", speed_mps(&alex));
    assert_eq!(alex.mode, Mode::Ground);
}

#[test]
fn sprint_ramps_to_full_speed() {
    let world = Boxes(vec![floor()]);
    let mut alex = standing(&world);
    run(&mut alex, forward(true), 0.5, &world);
    let early = speed_mps(&alex);
    run(&mut alex, forward(true), 3.0, &world);
    let late = speed_mps(&alex);
    assert!(early < 16.0, "early {early}");
    assert!((late - 20.0).abs() < 0.3, "late {late}");
}

fn apex_metres(alex: &mut Alex, input: Input, world: &Boxes) -> f32 {
    let p = Params::default();
    let start = alex.origin.z;
    let mut top = start;
    for _ in 0..600 {
        alex.step(&input, DT, &p, world);
        top = top.max(alex.origin.z);
        if alex.mode == Mode::Ground && alex.mode_time > 0.0 && top > start + 1.0 {
            break;
        }
    }
    (top - start) / UNITS_PER_METRE
}

#[test]
fn tapped_jump_on_the_move_leaves_on_release_at_jump_height() {
    let world = Boxes(vec![floor()]);
    let mut alex = standing(&world);
    run(&mut alex, forward(false), 0.3, &world);
    let p = Params::default();
    alex.step(&Input { jump: true, ..forward(false) }, DT, &p, &world);
    assert_eq!(alex.mode, Mode::Ground, "a press only starts the charge");
    let events = alex.step(&forward(false), DT, &p, &world);
    assert_eq!(events, vec![Event::Jumped]);
    let apex = apex_metres(&mut alex, forward(false), &world);
    assert!((apex - p.jump_height).abs() < p.jump_height * 0.06, "apex {apex}");
}

#[test]
fn jump_charges_while_running_without_slowing_down() {
    let world = Boxes(vec![floor()]);
    let mut alex = standing(&world);
    run(&mut alex, forward(false), 0.5, &world);
    run(&mut alex, Input { jump: true, ..forward(false) }, 1.2, &world);
    assert_eq!(alex.mode, Mode::Ground);
    assert!((speed_mps(&alex) - Params::default().run_speed).abs() < 0.2, "{}", speed_mps(&alex));
    let p = Params::default();
    let apex = apex_metres(&mut alex, forward(false), &world);
    assert!((apex - p.charged_jump_height).abs() < p.charged_jump_height * 0.04, "apex {apex}");
}

#[test]
fn full_charge_reaches_charged_height() {
    let world = Boxes(vec![floor()]);
    let mut alex = standing(&world);
    let hold = Input { jump: true, ..Input::default() };
    run(&mut alex, hold, 1.2, &world);
    assert_eq!(alex.mode, Mode::Ground, "charging keeps Alex on the ground");
    let p = Params::default();
    let apex = apex_metres(&mut alex, Input::default(), &world);
    assert!((apex - p.charged_jump_height).abs() < p.charged_jump_height * 0.04, "apex {apex}");
}

fn tap_jump(alex: &mut Alex, input: Input, world: &Boxes) {
    let p = Params::default();
    alex.step(&Input { jump: true, ..input }, DT, &p, world);
    alex.step(&input, DT, &p, world);
    assert_eq!(alex.mode, Mode::Air);
}

#[test]
fn second_jump_press_hops_then_glides_while_held() {
    let world = Boxes(vec![floor()]);
    let mut alex = standing(&world);
    let p = Params::default();
    tap_jump(&mut alex, forward(false), &world);
    run(&mut alex, forward(false), 0.3, &world);
    assert_eq!(alex.mode, Mode::Air, "no glide without a second press");
    let events = run(&mut alex, Input { jump: true, ..forward(false) }, 2.0, &world);
    assert_eq!(events.first(), Some(&Event::GlideHop));
    assert_eq!(alex.mode, Mode::Glide);
    assert!(alex.velocity.z >= -p.glide_max_sink * UNITS_PER_METRE - 0.5, "vz {}", alex.velocity.z);
    run(&mut alex, forward(false), 0.1, &world);
    assert_eq!(alex.mode, Mode::Air, "letting go ends the glide");
}

#[test]
fn run_in_the_air_dashes_twice_at_most() {
    let world = Boxes(vec![floor()]);
    let mut alex = standing(&world);
    let p = Params::default();
    let charge = Input { jump: true, ..Input::default() };
    run(&mut alex, charge, 1.2, &world);
    alex.step(&Input::default(), DT, &p, &world);
    run(&mut alex, Input::default(), 0.5, &world);
    let z = alex.origin.z;
    let events = alex.step(&forward(true), DT, &p, &world);
    assert_eq!(events, vec![Event::AirDash]);
    run(&mut alex, forward(true), 0.15, &world);
    assert!((speed_mps(&alex) - p.air_dash_speed).abs() < 0.5, "dash speed {}", speed_mps(&alex));
    assert!((alex.origin.z - z).abs() < 1.0, "the dash flies level");
    run(&mut alex, forward(false), 0.8, &world);
    assert!((speed_mps(&alex) - p.dash_brake_speed).abs() < 2.0, "braked to {}", speed_mps(&alex));
    let mut dashes = 0;
    for _ in 0..3 {
        dashes += alex.step(&forward(true), DT, &p, &world).iter().filter(|e| **e == Event::AirDash).count();
        run(&mut alex, forward(false), 0.1, &world);
    }
    assert_eq!(dashes, 1, "two dashes per time in the air");
}

#[test]
fn glide_hop_right_after_a_charged_jump_keeps_the_climb() {
    // Seen in game: a second press cut a full-charge jump to a third of its height.
    let world = Boxes(vec![floor()]);
    let mut alex = standing(&world);
    let p = Params::default();
    run(&mut alex, Input { jump: true, ..Input::default() }, 1.2, &world);
    alex.step(&Input::default(), DT, &p, &world);
    run(&mut alex, Input::default(), 0.1, &world);
    let climbing = alex.velocity.z;
    alex.step(&Input { jump: true, ..Input::default() }, DT, &p, &world);
    assert!(alex.velocity.z >= climbing - p.gravity * UNITS_PER_METRE * DT * 1.5, "{} < {climbing}", alex.velocity.z);
    let already = alex.origin.z / UNITS_PER_METRE;
    let apex = already + apex_metres(&mut alex, Input { jump: true, ..Input::default() }, &world);
    assert!(apex > p.charged_jump_height * 0.9, "apex {apex}");
}

#[test]
fn steps_up_one_block() {
    let block = (Vec3::new(200.0, -500.0, 0.0), Vec3::new(10000.0, 500.0, 36.0));
    let world = Boxes(vec![floor(), block]);
    let mut alex = standing(&world);
    run(&mut alex, forward(false), 2.0, &world);
    assert!(alex.origin.x > 300.0, "x {}", alex.origin.x);
    assert!((alex.origin.z - 36.0).abs() < 2.0, "z {}", alex.origin.z);
}

#[test]
fn sprint_into_a_wall_runs_up_it_and_vaults_over() {
    let wall = (Vec3::new(400.0, -500.0, 0.0), Vec3::new(10000.0, 500.0, 720.0));
    let world = Boxes(vec![floor(), wall]);
    let mut alex = standing(&world);
    let events = run(&mut alex, forward(true), 6.0, &world);
    let start = events.iter().position(|e| *e == Event::WallStart);
    let vault = events.iter().position(|e| *e == Event::Vault);
    assert!(start.is_some() && vault > start, "{events:?}");
    assert_eq!(alex.mode, Mode::Ground);
    assert!(alex.origin.z > 700.0 && alex.origin.x > 420.0, "on top: {}", alex.origin);
}

#[test]
fn letting_go_of_sprint_drops_off_the_wall() {
    let wall = (Vec3::new(400.0, -500.0, 0.0), Vec3::new(10000.0, 500.0, 7200.0));
    let world = Boxes(vec![floor(), wall]);
    let mut alex = standing(&world);
    run(&mut alex, forward(true), 3.0, &world);
    assert_eq!(alex.mode, Mode::WallRun);
    run(&mut alex, forward(false), 0.1, &world);
    assert_eq!(alex.mode, Mode::Air);
}

fn tall_wall() -> Boxes {
    let wall = (Vec3::new(400.0, -500.0, 0.0), Vec3::new(10000.0, 500.0, 72000.0));
    Boxes(vec![floor(), wall])
}

#[test]
fn jump_on_a_wall_with_the_stick_pulled_away_pushes_off() {
    let world = tall_wall();
    let mut alex = standing(&world);
    run(&mut alex, forward(true), 3.0, &world);
    assert_eq!(alex.mode, Mode::WallRun);
    let p = Params::default();
    let away = Input { move_axis: [-1.0, 0.0], sprint: true, ..Input::default() };
    alex.step(&Input { jump: true, ..away }, DT, &p, &world);
    let events = alex.step(&away, DT, &p, &world);
    assert_eq!(events, vec![Event::WallJump]);
    assert!(alex.velocity.x < 0.0 && alex.velocity.z > 0.0);
}

fn rise_metres(alex: &mut Alex, input: Input, seconds: f32, world: &Boxes) -> f32 {
    let z = alex.origin.z;
    run(alex, input, seconds, world);
    (alex.origin.z - z) / UNITS_PER_METRE
}

#[test]
fn jump_on_a_wall_leaps_straight_up_it() {
    let world = tall_wall();
    let p = Params::default();
    let mut plain = standing(&world);
    run(&mut plain, forward(true), 3.0, &world);
    let mut leaping = plain.clone();
    let climbed = rise_metres(&mut plain, forward(true), 1.0, &world);
    leaping.step(&Input { jump: true, ..forward(true) }, DT, &p, &world);
    let events = leaping.step(&forward(true), DT, &p, &world);
    assert_eq!(events, vec![Event::WallLeap]);
    let leapt = rise_metres(&mut leaping, forward(true), 1.0, &world);
    assert_eq!(leaping.mode, Mode::WallRun, "still on the wall");
    assert!((leaping.origin.x - plain.origin.x).abs() < 2.0, "straight up, not off the wall");
    assert!(leapt > climbed + 2.0, "leap {leapt} m against {climbed} m");
}

#[test]
fn jump_charges_on_a_wall_too() {
    let world = tall_wall();
    let p = Params::default();
    let mut alex = standing(&world);
    run(&mut alex, forward(true), 3.0, &world);
    run(&mut alex, Input { jump: true, ..forward(true) }, 1.2, &world);
    assert_eq!(alex.mode, Mode::WallRun, "charging keeps running up the wall");
    let mut tapped = alex.clone();
    tapped.charge = Some(0.0);
    alex.step(&forward(true), DT, &p, &world);
    tapped.step(&forward(true), DT, &p, &world);
    let charged = rise_metres(&mut alex, forward(true), 2.5, &world);
    let tap = rise_metres(&mut tapped, forward(true), 2.5, &world);
    assert!(charged > tap + 10.0, "charged {charged} m, tapped {tap} m");
}

/// Seen in game: a wall to the side under a two-block ceiling.
fn low_corridor() -> Boxes {
    let wall = (Vec3::new(-10000.0, 300.0, 0.0), Vec3::new(10000.0, 10000.0, 10000.0));
    let ceiling = (Vec3::new(-10000.0, -10000.0, 76.0), Vec3::new(10000.0, 10000.0, 10000.0));
    Boxes(vec![floor(), wall, ceiling])
}

#[test]
fn grazing_a_wall_at_45_degrees_does_not_take_it() {
    let world = low_corridor();
    let mut alex = standing(&world);
    let diagonal = Input { move_axis: [1.0, 0.0], view_yaw: 45.0, sprint: true, jump: false };
    let events = run(&mut alex, diagonal, 3.0, &world);
    assert!(!events.contains(&Event::WallStart), "{events:?}");
    assert_eq!(alex.mode, Mode::Ground);
}

#[test]
fn a_low_ceiling_does_not_flicker_between_wall_and_ground() {
    let world = low_corridor();
    let mut alex = standing(&world);
    let into = Input { move_axis: [1.0, 0.0], view_yaw: 90.0, sprint: true, jump: false };
    let events = run(&mut alex, into, 3.0, &world);
    let starts = events.iter().filter(|e| **e == Event::WallStart).count();
    assert!(starts <= 7, "{starts} wall starts in 3 s");
}

#[test]
fn inside_blocks_climbs_out() {
    // Seen in game: stuck still with the stick held, after blocks streamed in around Alex.
    let mound = (Vec3::new(-200.0, -200.0, 0.0), Vec3::new(200.0, 200.0, 72.0));
    let world = Boxes(vec![floor(), mound]);
    let mut alex = Alex::new(Vec3::new(0.0, 0.0, 10.0), 0.0);
    let p = Params::default();
    let events = alex.step(&forward(false), DT, &p, &world);
    assert_eq!(events.first(), Some(&Event::Unstuck));
    run(&mut alex, Input::default(), 1.0, &world);
    assert!(alex.origin.z >= 71.0 && alex.mode == Mode::Ground, "{} {:?}", alex.origin, alex.mode);
}

#[test]
fn two_glide_hops_per_jump() {
    let world = Boxes(vec![floor()]);
    let mut alex = standing(&world);
    run(&mut alex, Input { jump: true, ..Input::default() }, 1.2, &world);
    let p = Params::default();
    alex.step(&Input::default(), DT, &p, &world);
    let held = Input { jump: true, ..forward(false) };
    let mut hops = 0;
    for _ in 0..3 {
        hops += run(&mut alex, held, 0.3, &world).iter().filter(|e| **e == Event::GlideHop).count();
        assert_eq!(alex.mode, Mode::Glide);
        run(&mut alex, forward(false), 0.1, &world);
    }
    assert_eq!(hops, 2);
}
