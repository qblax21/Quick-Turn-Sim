// SPDX-License-Identifier: Apache-2.0
//! Headless simulation binary. A thin command-line wrapper around the
//! `qts_sim` library; it contains no simulation logic.

use qts_core::constants::{build_id, PRODUCT_NAME};
use qts_core::input::InputFrame;
use qts_core::replay::{Replay, ReplayRecorder};
use qts_core::snapshot::state_hash;
use qts_core::step::Simulation;
use qts_ffb::{FfbPipeline, FfbSample};
use qts_sim::data::{provenance_report, DataRoot};
use qts_sim::drivers::{Driver, Erratic, Idle, PathFollower, PathFollowerConfig, Scripted};
use qts_sim::host::ffb_input;
use qts_sim::session::{Session, SessionAssets, SessionConfig};
use qts_sim::telemetry;
use qts_telemetry::import::{import_csv, summarize, targets_snippet};
use qts_telemetry::TelemetryWriter;
use qts_vehicle::Assists;
use std::collections::BTreeMap;
use std::io::{BufWriter, Write};
use std::process::ExitCode;

const USAGE: &str = "\
Commands:
  list                                   cars, tracks, tires, surfaces, presets
  run --car ID --track ID [options]      run a session without a window
      --seconds N          simulated time to run (default 10)
      --driver NAME        idle | launch | path | erratic (default launch)
      --seed N             seed for the erratic driver (default 1)
      --rate HZ            physics rate (default 1000)
      --tire-temp C        starting tire temperature (default: road temperature)
      --replay FILE        record a replay
      --telemetry FILE     write telemetry CSV
      --telemetry-rate HZ  telemetry rows per second (default 100)
      --hash-log FILE      write `tick hash` every 100 ticks
      --ffb PRESET         force-feedback preset used for the FFB channels
  replay FILE                            re-simulate a replay and compare hashes
  provenance --car ID [--track ID]       where every parameter came from
  track-info ID                          length, bounds and surfaces of a track
  import FILE --profile ID [--targets]   read a logger CSV and summarise it
  channels                               telemetry channel table (Markdown)
  ffb-devices                            list steering devices
  ffb-test --device ID [--seconds N] [--amplitude A]
                                         send a slow sine to a device
";

/// Parsed command line: positional arguments and `--key value` options.
struct Args {
    positional: Vec<String>,
    options: BTreeMap<String, String>,
}

impl Args {
    fn parse(raw: &[String]) -> Result<Self, String> {
        let mut positional = Vec::new();
        let mut options = BTreeMap::new();
        let mut i = 0;
        while i < raw.len() {
            if let Some(key) = raw[i].strip_prefix("--") {
                // A flag with no value is stored as "true".
                let has_value = raw.get(i + 1).is_some_and(|v| !v.starts_with("--"));
                let value = if has_value {
                    i += 1;
                    raw[i].clone()
                } else {
                    "true".to_string()
                };
                options.insert(key.to_string(), value);
            } else {
                positional.push(raw[i].clone());
            }
            i += 1;
        }
        Ok(Self {
            positional,
            options,
        })
    }

    fn text(&self, key: &str) -> Option<&str> {
        self.options.get(key).map(|s| s.as_str())
    }

    fn required(&self, key: &str) -> Result<&str, String> {
        self.text(key).ok_or_else(|| format!("missing --{key}"))
    }

    fn number(&self, key: &str, default: f64) -> Result<f64, String> {
        match self.text(key) {
            None => Ok(default),
            Some(v) => v
                .parse::<f64>()
                .ok()
                .filter(|x| x.is_finite())
                .ok_or_else(|| format!("--{key} needs a number, got `{v}`")),
        }
    }
}

fn data_root() -> Result<DataRoot, String> {
    DataRoot::discover().map_err(|e| e.to_string())
}

fn cmd_list() -> Result<(), String> {
    let root = data_root()?;
    println!("{PRODUCT_NAME} data at {}", root.path().display());
    for (kind, title) in [
        ("cars", "Cars"),
        ("tracks", "Tracks"),
        ("tires", "Tires"),
        ("surfaces", "Surfaces"),
        ("ffb", "Force-feedback presets"),
        ("import", "Import profiles"),
        ("validation", "Validation target sets"),
    ] {
        println!("{title}:");
        for id in root.list(kind).unwrap_or_default() {
            let label = match kind {
                "cars" => root
                    .load_car(&id)
                    .map(|c| c.value.name)
                    .map_err(|e| e.to_string()),
                "tracks" => root
                    .load_track(&id)
                    .map(|t| t.0.name)
                    .map_err(|e| e.to_string()),
                "tires" => root
                    .load_tire(&id)
                    .map(|t| t.value.name)
                    .map_err(|e| e.to_string()),
                "surfaces" => root
                    .load_surface(&id)
                    .map(|s| {
                        if s.value.tuned {
                            s.value.name
                        } else {
                            format!("{} [untuned]", s.value.name)
                        }
                    })
                    .map_err(|e| e.to_string()),
                "ffb" => root
                    .load_ffb(&id)
                    .map(|f| {
                        if f.value.tuned {
                            f.value.name
                        } else {
                            format!("{} [untuned]", f.value.name)
                        }
                    })
                    .map_err(|e| e.to_string()),
                _ => Ok(String::new()),
            };
            match label {
                Ok(name) => println!("  {id:<28} {name}"),
                Err(e) => println!("  {id:<28} ERROR: {e}"),
            }
        }
    }
    Ok(())
}

fn make_driver(name: &str, session: &Session, seed: u64) -> Result<Box<dyn Driver>, String> {
    let rate = session.tick_rate_hz();
    match name {
        "idle" => Ok(Box::new(Idle)),
        "erratic" => Ok(Box::new(Erratic::new(seed))),
        "launch" => Ok(Box::new(Scripted::new(
            rate,
            &[
                (0.5, InputFrame::default()),
                (8.0, InputFrame::from_analog(0.0, 1.0, 0.0, 0.0, 0.0)),
                (60.0, InputFrame::from_analog(0.0, 0.0, 1.0, 0.0, 0.0)),
            ],
        ))),
        "path" => {
            let path = session
                .track()
                .paths()
                .first()
                .ok_or("this track has no path for the path driver to follow")?;
            Ok(Box::new(PathFollower::new(
                path,
                PathFollowerConfig::default(),
            )))
        }
        other => Err(format!(
            "unknown driver `{other}` (idle, launch, path, erratic)"
        )),
    }
}

fn cmd_run(args: &Args) -> Result<(), String> {
    let root = data_root()?;
    let car_id = args.required("car")?;
    let track_id = args.required("track")?;
    let seconds = args.number("seconds", 10.0)?;
    let driver_name = args.text("driver").unwrap_or("launch");
    let config = SessionConfig {
        tick_rate_hz: args.number("rate", 1000.0)? as u32,
        // Scripted drivers do not operate a clutch or gear lever.
        assists: Assists {
            auto_clutch: true,
            auto_shift: true,
            traction_control: true,
        },
        tire_start_temp_c: match args.text("tire-temp") {
            Some(_) => Some(args.number("tire-temp", 0.0)?),
            None => None,
        },
    };
    let mut session = Session::from_data(&root, car_id, track_id, config)?;
    let mut driver = make_driver(driver_name, &session, args.number("seed", 1.0)? as u64)?;
    let rate = session.tick_rate_hz();
    let ticks = (seconds * f64::from(rate)).round().max(0.0) as u64;

    let ffb_preset = args.text("ffb").unwrap_or("generic_wheel");
    let ffb_config = root.load_ffb(ffb_preset).map_err(|e| e.to_string())?.value;
    let mut pipeline = FfbPipeline::new(ffb_config, rate)?;

    let mut recorder = args
        .text("replay")
        .map(|_| ReplayRecorder::start(session.replay_header(100), &session));
    let mut telemetry_out = match args.text("telemetry") {
        Some(path) => {
            let sample_rate = args.number("telemetry-rate", 100.0)?.max(1.0);
            let decimation = (f64::from(rate) / sample_rate).round().max(1.0) as u32;
            let file = std::fs::File::create(path).map_err(|e| format!("{path}: {e}"))?;
            Some(
                TelemetryWriter::new(
                    BufWriter::new(file),
                    &telemetry::generator(),
                    &telemetry::metadata(&session, f64::from(rate) / f64::from(decimation)),
                    &telemetry::channels(),
                    decimation,
                )
                .map_err(|e| format!("{path}: {e}"))?,
            )
        }
        None => None,
    };
    let mut hash_log = match args.text("hash-log") {
        Some(path) => Some(BufWriter::new(
            std::fs::File::create(path).map_err(|e| format!("{path}: {e}"))?,
        )),
        None => None,
    };

    // The wall clock is read here only to report how fast the run went.
    let started = std::time::Instant::now();
    let mut row = Vec::new();
    let mut max_speed: f64 = 0.0;
    let mut sample = FfbSample::default();
    for _ in 0..ticks {
        let frame = driver.frame(&session);
        session.step(&frame);
        if !session.is_finite() {
            return Err(format!(
                "state became non-finite at tick {}",
                session.tick()
            ));
        }
        max_speed = max_speed.max(session.car().output().speed);
        if telemetry_out.is_some() {
            sample = pipeline.step(&ffb_input(&session, None));
        }
        if let Some(writer) = telemetry_out.as_mut() {
            telemetry::fill_row(&session, &frame, &sample, &mut row);
            writer.sample(&row).map_err(|e| format!("telemetry: {e}"))?;
        }
        if let Some(rec) = recorder.as_mut() {
            rec.record(frame, &session);
        }
        if let Some(log) = hash_log.as_mut() {
            if session.tick() % 100 == 0 {
                writeln!(log, "{} {:016x}", session.tick(), state_hash(&session))
                    .map_err(|e| format!("hash log: {e}"))?;
            }
        }
    }
    let wall = started.elapsed().as_secs_f64();

    let out = session.car().output();
    let pos = session.car().state.pos;
    println!("car        {}", session.car().params().name);
    println!("track      {}", session.track().def().name);
    println!("driver     {driver_name}");
    println!("build      {}", build_id());
    println!(
        "ticks      {} ({:.3} s at {} Hz)",
        session.tick(),
        session.sim_time(),
        rate
    );
    println!(
        "position   x {:.2} m, y {:.2} m, z {:.3} m",
        pos.x, pos.y, pos.z
    );
    println!(
        "speed      {:.2} m/s now, {:.2} m/s peak",
        out.speed, max_speed
    );
    println!(
        "gear       {}, {:.0} rpm",
        out.drive.gear, out.drive.engine_rpm
    );
    let timing = session.timing();
    if timing.runs > 0 {
        println!(
            "timed run  {:.3} s, {} cones",
            session.ticks_to_seconds(timing.last_ticks),
            timing.last_cones_hit
        );
    }
    println!("state hash {:016x}", state_hash(&session));
    if wall > 0.0 {
        println!(
            "wall time  {:.2} s ({:.0}x real time)",
            wall,
            session.sim_time() / wall
        );
    }
    if let Some(writer) = telemetry_out {
        let rows = writer.rows();
        writer.finish().map_err(|e| format!("telemetry: {e}"))?;
        println!(
            "telemetry  {} rows -> {}",
            rows,
            args.text("telemetry").unwrap_or("")
        );
    }
    if let (Some(rec), Some(path)) = (recorder, args.text("replay")) {
        let replay = rec.finish();
        std::fs::write(path, replay.to_bytes()).map_err(|e| format!("{path}: {e}"))?;
        println!("replay     {} frames -> {path}", replay.frames.len());
    }
    if let Some(mut log) = hash_log {
        log.flush().map_err(|e| format!("hash log: {e}"))?;
    }
    Ok(())
}

fn cmd_replay(args: &Args) -> Result<bool, String> {
    let path = args.positional.get(1).ok_or("replay needs a file")?;
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let replay = Replay::from_bytes(&bytes).map_err(|e| format!("{path}: {e}"))?;
    let h = &replay.header;
    println!("replay     {path}");
    println!("car        {}", h.car_id);
    println!("track      {}", h.track_id);
    println!(
        "recorded   {} ticks at {} Hz by build {}",
        replay.frames.len(),
        h.tick_rate_hz,
        h.build_id
    );
    let root = data_root()?;
    let config = SessionConfig {
        tick_rate_hz: h.tick_rate_hz,
        ..SessionConfig::default()
    };
    let mut session = Session::from_data(&root, &h.car_id, &h.track_id, config)?;
    if session.content_hash() != h.content_hash {
        return Err(format!(
            "the data files have changed since this replay was recorded (content hash {:016x}, replay has {:016x})",
            session.content_hash(),
            h.content_hash
        ));
    }
    if h.build_id != build_id() {
        println!(
            "WARNING    this build is {}; float results are only guaranteed to match within one build",
            build_id()
        );
    }
    let report = replay.verify(&mut session).map_err(|e| e.to_string())?;
    println!(
        "replayed   {} ticks, {} hash checkpoints",
        report.ticks, report.checkpoints
    );
    println!("final hash {:016x}", report.final_hash);
    match report.first_mismatch {
        None => {
            println!("result     EXACT: every checkpoint matched");
            Ok(true)
        }
        Some((tick, recorded, now)) => {
            println!(
                "result     DIVERGED at tick offset {tick}: recorded {recorded:016x}, replayed {now:016x}"
            );
            Ok(false)
        }
    }
}

fn cmd_provenance(args: &Args) -> Result<(), String> {
    let root = data_root()?;
    let car_id = args.required("car")?;
    let track_id = args.text("track").unwrap_or("test_track");
    let assets = SessionAssets::load(&root, car_id, track_id).map_err(|e| e.to_string())?;
    print!(
        "{}",
        provenance_report(
            &format!("{} on {}", assets.car.name, track_id),
            &assets.provenance
        )
    );
    Ok(())
}

fn cmd_track_info(args: &Args) -> Result<(), String> {
    let root = data_root()?;
    let id = args
        .positional
        .get(1)
        .ok_or("track-info needs a track id")?;
    // Any car will do; only the track is inspected.
    let car = root
        .list("cars")
        .map_err(|e| e.to_string())?
        .into_iter()
        .next()
        .ok_or("no cars in the data directory")?;
    let assets = SessionAssets::load(&root, &car, id).map_err(|e| e.to_string())?;
    let track = &assets.track;
    println!("{} ({})", track.def().name, track.def().id);
    println!("{}", track.def().description);
    println!(
        "cones {}   gates {}",
        track.cones().len(),
        track.gates().len()
    );
    for path in track.paths() {
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for p in &path.points {
            min_x = min_x.min(p[0]);
            min_y = min_y.min(p[1]);
            max_x = max_x.max(p[0]);
            max_y = max_y.max(p[1]);
        }
        // Closest approach between parts of the path at least 30 m apart
        // along its length: a small value means the layout overlaps itself.
        let last = path.points[path.points.len() - 1];
        let closed =
            (last[0] - path.points[0][0]).abs() < 0.5 && (last[1] - path.points[0][1]).abs() < 0.5;
        let mut closest = f64::INFINITY;
        for i in 0..path.points.len() {
            for j in (i + 1)..path.points.len() {
                let apart = path.s[j] - path.s[i];
                // On a closed lap the two ends are neighbours too.
                if apart < 30.0 || (closed && path.length() - apart < 30.0) {
                    continue;
                }
                let (dx, dy) = (
                    path.points[i][0] - path.points[j][0],
                    path.points[i][1] - path.points[j][1],
                );
                closest = closest.min((dx * dx + dy * dy).sqrt());
            }
        }
        println!(
            "path `{}`: {:.1} m, x {:.1}..{:.1}, y {:.1}..{:.1}, closest self-approach {:.1} m",
            path.id,
            path.length(),
            min_x,
            max_x,
            min_y,
            max_y,
            closest
        );
        let (end, end_heading) = path.at(path.length());
        println!(
            "        ends at ({:.3}, {:.3}) heading {:.3} deg; starts at ({:.3}, {:.3})",
            end[0],
            end[1],
            end_heading.to_degrees(),
            path.points[0][0],
            path.points[0][1]
        );
        if track.has_terrain() {
            // Height and steepest grade along the centre line.
            let (mut low, mut high, mut steepest) = (f64::INFINITY, f64::NEG_INFINITY, 0.0f64);
            let mut previous = track.ground_height(path.points[0][0], path.points[0][1]);
            for i in 0..path.points.len() {
                let h = track.ground_height(path.points[i][0], path.points[i][1]);
                low = low.min(h);
                high = high.max(h);
                if i > 0 && path.s[i] > path.s[i - 1] {
                    steepest = steepest.max(((h - previous) / (path.s[i] - path.s[i - 1])).abs());
                }
                previous = h;
            }
            println!(
                "        height {:.1}..{:.1} m, steepest grade {:.1} percent",
                low,
                high,
                steepest * 100.0
            );
        }
        // Surface along the path, as runs.
        let mut run_start = 0.0;
        let mut current: Option<u16> = None;
        let mut d = 0.0;
        while d <= path.length() {
            let (p, _) = path.at(d);
            let (surface, _) = track.surface_at(p[0], p[1]);
            if current != Some(surface) {
                if let Some(c) = current {
                    println!(
                        "    {:>7.1} .. {:>7.1} m  {}",
                        run_start,
                        d,
                        track.surfaces()[usize::from(c)].id
                    );
                }
                current = Some(surface);
                run_start = d;
            }
            d += 1.0;
        }
        if let Some(c) = current {
            println!(
                "    {:>7.1} .. {:>7.1} m  {}",
                run_start,
                path.length(),
                track.surfaces()[usize::from(c)].id
            );
        }
    }
    Ok(())
}

fn cmd_import(args: &Args) -> Result<(), String> {
    let path = args.positional.get(1).ok_or("import needs a CSV file")?;
    let root = data_root()?;
    let profile_id = args.text("profile").unwrap_or("generic");
    let profile = root
        .load_import_profile(profile_id)
        .map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    let run = import_csv(&text, &profile, name).map_err(|e| format!("{path}: {e}"))?;
    println!("file       {path}");
    println!("profile    {} ({})", profile.name, profile_id);
    println!("rows       {} kept, {} skipped", run.rows, run.skipped_rows);
    println!("channels:");
    for (channel, column) in &run.mapped_from {
        println!("  {channel:<20} <- column `{column}`");
    }
    let s = summarize(&run);
    println!(
        "duration   {:.2} s at about {:.1} Hz",
        s.duration_s, s.sample_rate_hz
    );
    let show = |label: &str, value: Option<f64>, unit: &str| match value {
        Some(v) => println!("{label:<26} {v:.3} {unit}"),
        None => println!("{label:<26} not found in this run"),
    };
    show("top speed", s.max_speed_m_s, "m/s");
    show("lateral, held 1 s", s.lateral_g_sustained, "g");
    show("braking, held 0.5 s", s.braking_g_sustained, "g");
    show("0-60 mph", s.zero_to_sixty_s, "s");
    show("60-0 mph distance", s.sixty_to_zero_m, "m");
    if args.text("targets").is_some() {
        println!("\n# Paste into a file under data/validation/ and review before use.");
        print!("{}", targets_snippet(&s, name));
    }
    Ok(())
}

fn cmd_ffb_devices() -> Result<(), String> {
    let devices = qts_ffb::platform::enumerate().map_err(|e| e.to_string())?;
    if devices.is_empty() {
        println!("no steering devices found");
    }
    for d in devices {
        println!(
            "{}  {}  [{}{}]",
            d.id,
            d.name,
            d.backend,
            if d.has_force_feedback {
                ", force feedback"
            } else {
                ", no force feedback"
            }
        );
    }
    Ok(())
}

fn cmd_ffb_test(args: &Args) -> Result<(), String> {
    let id = args.required("device")?;
    let seconds = args.number("seconds", 4.0)?.clamp(0.5, 30.0);
    let amplitude = args.number("amplitude", 0.15)?.clamp(0.0, 0.5);
    let mut device = qts_ffb::platform::open(id, None).map_err(|e| e.to_string())?;
    println!("opened {}", device.info().name);
    println!(
        "sending a 0.5 Hz sine at {:.0}% of device torque for {seconds:.1} s; keep hands clear",
        amplitude * 100.0
    );
    let started = std::time::Instant::now();
    loop {
        let t = started.elapsed().as_secs_f64();
        if t >= seconds {
            break;
        }
        let torque = amplitude * (2.0 * std::f64::consts::PI * 0.5 * t).sin();
        device
            .set_torque(torque as f32)
            .map_err(|e| e.to_string())?;
        if let Ok(Some(axis)) = device.read_steering() {
            print!("\rcommand {torque:+.3}   steering axis {axis:+.3}   ");
            let _ = std::io::stdout().flush();
        }
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
    device.stop().map_err(|e| e.to_string())?;
    println!("\ndone. If the wheel moved right while the command was positive, set `invert = true` in the preset.");
    Ok(())
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(&raw) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let command = args.positional.first().map(|s| s.as_str()).unwrap_or("");
    let result = match command {
        "list" => cmd_list().map(|()| true),
        "run" => cmd_run(&args).map(|()| true),
        "replay" => cmd_replay(&args),
        "provenance" => cmd_provenance(&args).map(|()| true),
        "track-info" => cmd_track_info(&args).map(|()| true),
        "import" => cmd_import(&args).map(|()| true),
        "channels" => {
            println!("| Column | Unit | Meaning |");
            println!("| --- | --- | --- |");
            for c in telemetry::channels() {
                println!("| `{}` | {} | {} |", c.name, c.unit, c.description);
            }
            Ok(true)
        }
        "ffb-devices" => cmd_ffb_devices().map(|()| true),
        "ffb-test" => cmd_ffb_test(&args).map(|()| true),
        "" | "help" | "--help" | "-h" => {
            println!("{PRODUCT_NAME} headless simulator\n\n{USAGE}");
            Ok(true)
        }
        other => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
