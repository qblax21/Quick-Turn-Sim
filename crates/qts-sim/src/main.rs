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
use qts_sim::host::{ffb_input, HostOptions, SteppedHost};
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
      --ffb-log FILE       log the force-feedback signal for the first full lap
      --lap-telemetry DIR  write every lap to its own telemetry file in DIR
  bench [--car ID] [--track ID] [--seconds N] [--driver NAME]
                                         time the host's tick (input, physics and
                                         force feedback) as fast as it will run;
                                         every car on Ridgeline by default
  replay FILE                            re-simulate a replay and compare hashes
  provenance --car ID [--track ID]       where every parameter came from
  track-info ID                          length, bounds and surfaces of a track
  import FILE --profile ID [--targets]   read a logger CSV and summarise it
  channels                               telemetry channel table (Markdown)
  ffb-report FILE...                     clipping and grip-limit summary of FFB logs
  kerb-ffb [--car ID] [--ffb PRESET] [--csv FILE]
                                         force feedback over a Ridgeline kerb,
                                         with and without kerb normals
  steering-compare [--csv FILE]          column torque against speed on a
                                         Ridgeline lap and the skidpad, with
                                         the steering model before D-075 and now
  ffb-gain [--wheel ID] [--knee K]       column torque over scripted Ridgeline
                                         laps of every car, and the gain that
                                         puts it at the knee of a wheel base
  lap-compare A.csv B.csv [--svg FILE] [--step M]
                                         line two lap files up by distance:
                                         where B gains and loses time on A
  setup-sheet --car ID [--setup FILE] [--track ID]
                                         a setup in real units with its ride
                                         frequencies, damping, roll split and
                                         differential locking; loads back
  cold-start [--car ID] [--track ID] [--cold C] [--warm C] [--accel A] [--svg FILE]
                                         first lap from the pits with cold and
                                         warm tires: lap times, grip and
                                         temperatures corner by corner
  ai --car ID --track ID [--mode grip|drift] [--pace P] [--laps N] [--replay FILE]
                                         the computer driver (D-122): no
                                         assists, own clutch and shifts; prints
                                         lap times and drift scores
  coach LAP.csv BEST.csv                  the lap corner by corner against the
                                         best: time lost per corner and why
  tire-plots [--out DIR]                 force against slip at three loads and
                                         the combined-slip envelope, one SVG
                                         per tire file (default docs/img)
  ffb-devices                            list steering devices
  input-read [--device ID] [--reads N]   read every axis and button of the
                                         attached controllers and time the reads
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
            // A circuit's path ends where it starts: keep lapping it.
            let (first, last) = (path.points[0], path.points[path.points.len() - 1]);
            let closed = (first[0] - last[0]).hypot(first[1] - last[1]) < 30.0;
            Ok(Box::new(PathFollower::new(
                path,
                PathFollowerConfig {
                    closed,
                    ..PathFollowerConfig::default()
                },
            )))
        }
        other => Err(format!(
            "unknown driver `{other}` (idle, launch, path, erratic)"
        )),
    }
}

/// Times the host's per-tick work: the same input ramp, physics step and
/// force-feedback pipeline the real-time host runs, through
/// [`SteppedHost`], one tick per call and as fast as it will go. The wall
/// clock is read here only to measure.
fn cmd_bench(args: &Args) -> Result<(), String> {
    let root = data_root()?;
    let seconds = args.number("seconds", 60.0)?;
    let track_id = args.text("track").unwrap_or("ridgeline_circuit");
    let driver_name = args.text("driver").unwrap_or("path");
    let cars: Vec<String> = match args.text("car") {
        Some(c) => vec![c.to_string()],
        None => root.list("cars").map_err(|e| e.to_string())?,
    };
    let ffb = root
        .load_ffb(args.text("ffb").unwrap_or("generic_wheel"))
        .map_err(|e| e.to_string())?
        .value;
    let mut printed_rate = false;
    for car in &cars {
        let config = SessionConfig {
            assists: Assists {
                auto_clutch: true,
                auto_shift: true,
                traction_control: true,
                engine_damage: false,
            },
            ..SessionConfig::default()
        };
        let session = Session::from_data(&root, car, track_id, config)?;
        let rate = session.tick_rate_hz();
        if !printed_rate {
            println!(
                "physics and force feedback: fixed {rate} Hz on the host thread, apart from rendering; tick budget {:.0} µs",
                1e6 / f64::from(rate)
            );
            println!(
                "{:<20} {:>8} {:>9} {:>8} {:>8} {:>8} {:>8} {:>11}",
                "car",
                "ticks",
                "mean µs",
                "p50 µs",
                "p99 µs",
                "p99.9 µs",
                "worst µs",
                "x real time"
            );
            printed_rate = true;
        }
        let mut driver = make_driver(driver_name, &session, 1)?;
        let mut host = SteppedHost::new(
            session,
            HostOptions {
                ffb: ffb.clone(),
                max_catch_up_ticks: 100,
                interpolate_inputs: true,
                ghost_path: None,
            },
        )?;
        let ticks = (seconds * f64::from(rate)).round().max(1.0) as usize;
        let dt = 1.0 / f64::from(rate);
        let mut times = Vec::with_capacity(ticks);
        for _ in 0..ticks {
            let frame = driver.frame(host.session());
            host.set_input(frame);
            let began = std::time::Instant::now();
            let n = host.advance(dt).len();
            times.push(began.elapsed().as_secs_f64() * 1e6);
            debug_assert!(n <= 1);
        }
        let total: f64 = times.iter().sum();
        times.sort_by(f64::total_cmp);
        let pct = |p: f64| times[((times.len() - 1) as f64 * p).round() as usize];
        println!(
            "{:<20} {:>8} {:>9.1} {:>8.1} {:>8.1} {:>8.1} {:>8.1} {:>11.1}",
            car,
            ticks,
            total / ticks as f64,
            pct(0.5),
            pct(0.99),
            pct(0.999),
            times[times.len() - 1],
            ticks as f64 * dt / (total * 1e-6)
        );
    }
    Ok(())
}

fn cmd_setup_sheet(args: &Args) -> Result<(), String> {
    use qts_sim::setup::{setup_sheet, Setup};
    let root = data_root()?;
    let car = root
        .load_car(args.required("car")?)
        .map_err(|e| e.to_string())?
        .value;
    let tf = root
        .load_tire(&car.tire_front)
        .map_err(|e| e.to_string())?
        .value;
    let tr = root
        .load_tire(&car.tire_rear)
        .map_err(|e| e.to_string())?
        .value;
    let setup = match args.text("setup") {
        Some(path) => {
            Setup::from_text(&std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?)?
        }
        None => Setup::default(),
    };
    print!(
        "{}",
        setup_sheet(&car, &tf, &tr, &setup, args.text("track").unwrap_or(""))?
    );
    Ok(())
}

fn cmd_cold_start(args: &Args) -> Result<(), String> {
    use qts_sim::cold_start::{change, drive, svg, warm_up};
    let root = data_root()?;
    let car = args.text("car").unwrap_or("hatch_tern_gt");
    let track = args.text("track").unwrap_or("ridgeline_circuit");
    let cold_c = args.number("cold", 20.0)?;
    let warm_c = args.number("warm", 45.0)?;
    let accel = args.number("accel", 8.0)?;
    let cold = drive(&root, car, track, cold_c, accel, 2)?;
    let warm = drive(&root, car, track, warm_c, accel, 2)?;
    let w = warm_up(&cold[0], &warm[0], 0.005)?;
    let params = root.load_car(car).map_err(|e| e.to_string())?.value;
    let tire = root
        .load_tire(&params.tire_front)
        .map_err(|e| e.to_string())?
        .value;
    let (g, k) = change(&tire, 30.0, 70.0);
    println!("model, 30 to 70 C: grip {:+.1} %, stiffness {:+.1} % (published: grip about -10 %, stiffness -20 to -25 %)", 100.0 * g, 100.0 * k);
    println!(
        "lap 1 from the pits: {:.3} s from {cold_c:.0} C, {:.3} s from {warm_c:.0} C ({:+.3} s); lap 2: {:.3} s and {:.3} s",
        cold[0].lap_s,
        warm[0].lap_s,
        cold[0].lap_s - warm[0].lap_s,
        cold[1].lap_s,
        warm[1].lap_s
    );
    for (i, c) in w.corners.iter().enumerate() {
        let (gc, gw) = w.grip[i];
        let t = w.cold_temps[i];
        println!(
            "T{:<2} exit: cold grip {:.4}, warm {:.4} ({:+.2} %); cold tires surface {:.1} core {:.1} carcass {:.1} C",
            c.number,
            gc,
            gw,
            100.0 * (gc / gw - 1.0),
            t[0],
            t[1],
            t[2]
        );
    }
    match w.warm_after {
        Some(n) => println!(
            "within 0.5 % of the warm tires' grip from corner {} on",
            n + 1
        ),
        None => println!("not within 0.5 % of the warm tires' grip by the end of the lap"),
    }
    if let Some(path) = args.text("svg") {
        let title = format!(
            "{} on {}: cold tires on the first lap from the pits",
            params.name, track
        );
        std::fs::write(path, svg(&tire, &cold[0], &warm[0], &w, &title))
            .map_err(|e| format!("{path}: {e}"))?;
        println!("wrote {path}");
    }
    Ok(())
}

fn cmd_ai(args: &Args) -> Result<(), String> {
    use qts_sim::ai::{run_laps, AiConfig, AiDriver};
    let root = data_root()?;
    let car = args.required("car")?;
    let track = args.required("track")?;
    let pace = args.number("pace", 0.9)?;
    let laps = args.number("laps", 2.0)? as u32;
    let config = match args.text("mode").unwrap_or("grip") {
        "grip" => AiConfig::grip(pace),
        "drift" => {
            let mut c = AiConfig::drift(pace);
            c.drift_angle_deg = args.number("angle", c.drift_angle_deg)?;
            c
        }
        other => return Err(format!("unknown mode `{other}` (grip or drift)")),
    };
    let mut session = Session::from_data(&root, car, track, AiDriver::session_config())?;
    let mut ai = AiDriver::new(session.track(), config)?;
    // An out lap to the line, then the timed laps.
    let run = run_laps(&mut session, &mut ai, laps + 1, 120.0 * f64::from(laps + 1));
    for (i, (time, counted, score)) in run.laps.iter().enumerate() {
        let label = if i == 0 {
            "out lap".to_string()
        } else {
            format!("lap {i}")
        };
        let score = score.map_or(String::new(), |s| format!("   drift score {s:.1}"));
        println!(
            "{label:<8} {}:{:06.3}{}{score}",
            (*time / 60.0) as u32,
            time % 60.0,
            if *counted { "" } else { "  (not counted)" }
        );
    }
    for (name, score, why) in &run.zones {
        println!("  {name:<4} {score:5.1}  {why}");
    }
    if let Some(note) = &run.coach {
        println!("coach: {note}");
    }
    println!("{:.1} s driven, {} resets", run.seconds, ai.resets);
    Ok(())
}

fn cmd_coach(args: &Args) -> Result<(), String> {
    let [lap, best] = args
        .positional
        .get(1..3)
        .and_then(|p| <&[String; 2]>::try_from(p).ok())
        .ok_or("usage: coach LAP.csv BEST.csv (two per-lap telemetry files)")?;
    let lap = qts_sim::lap_telemetry::Lap::read(std::path::Path::new(lap))?;
    let best = qts_sim::lap_telemetry::Lap::read(std::path::Path::new(best))?;
    let report = qts_sim::coach::analyse(&lap, &best)?;
    print!("{}", qts_sim::coach::report_text(&report));
    Ok(())
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
            engine_damage: false,
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
    let mut ffb_log = args
        .text("ffb-log")
        .map(|path| qts_sim::ffb_log::FfbLogger::arm(path.into(), &session));
    let mut laps = match args.text("lap-telemetry") {
        Some(dir) => Some(qts_sim::lap_telemetry::LapRecorder::new(
            dir.as_ref(),
            &session,
        )?),
        None => None,
    };
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
        if telemetry_out.is_some() || ffb_log.is_some() || laps.is_some() {
            let input = ffb_input(&session, None);
            sample = pipeline.step(&input);
            if let Some(log) = ffb_log.as_mut() {
                log.observe(&session, &frame, &input, &sample, pipeline.config());
            }
        }
        if let Some(writer) = telemetry_out.as_mut() {
            telemetry::fill_row(&session, &frame, &sample, &mut row);
            writer.sample(&row).map_err(|e| format!("telemetry: {e}"))?;
        }
        if let Some(l) = laps.as_mut() {
            l.observe(&session, &frame, &sample);
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
    if let Some(mut log) = ffb_log {
        log.stop(&session, pipeline.config());
        println!("ffb log    {:?}", log.wait());
    }
    if let Some(mut l) = laps {
        for written in l.wait() {
            match written {
                Ok(p) => println!("lap        {}", p.display()),
                Err(e) => println!("lap        failed: {e}"),
            }
        }
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
    if !h.setup.is_empty() {
        println!("setup      {}", h.setup.trim_end().replace('\n', ", "));
    }
    let mut session = Session::for_replay(&root, h)?;
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

fn cmd_input_read(args: &Args) -> Result<(), String> {
    let reads = args.number("reads", 2000.0)?.max(1.0) as usize;
    let devices = qts_ffb::platform::enumerate().map_err(|e| e.to_string())?;
    if devices.is_empty() {
        println!("no game controllers attached");
    }
    for d in devices {
        if args.text("device").is_some_and(|id| id != d.id) {
            continue;
        }
        let mut reader = qts_ffb::platform::open_input(&d.id).map_err(|e| e.to_string())?;
        let mut state = reader.read_state().map_err(|e| e.to_string())?;
        let mut times: Vec<f64> = Vec::with_capacity(reads);
        let mut changes = 0usize;
        for _ in 0..reads {
            let start = std::time::Instant::now();
            let now = reader.read_state().map_err(|e| e.to_string())?;
            times.push(start.elapsed().as_secs_f64() * 1e6);
            if now != state {
                changes += 1;
            }
            state = now;
            // Spread the reads over time as the physics thread would.
            std::thread::sleep(std::time::Duration::from_micros(500));
        }
        times.sort_by(f64::total_cmp);
        println!("{}  {}", d.id, d.name);
        let present = reader.axes_present();
        for ((name, value), here) in qts_ffb::AXIS_NAMES.iter().zip(state.axes).zip(present) {
            if here {
                print!("  {name} {value:+.3}");
            }
        }
        print!("  ({} buttons)", reader.button_count());
        println!();
        let held: Vec<usize> = (0..qts_ffb::INPUT_BUTTONS)
            .filter(|b| state.button(*b))
            .collect();
        println!("  buttons held: {held:?}");
        println!(
            "  {} reads: median {:.1} us, 99th percentile {:.1} us, longest {:.1} us; state changed {} times",
            reads,
            times[reads / 2],
            times[(reads * 99) / 100],
            times[reads - 1],
            changes
        );
    }
    Ok(())
}

/// Drives the right wheels over Ridgeline's first kerb (the inside of T1)
/// twice, with the kerb's slope tilting the contact normal and without, and
/// compares the steering torque and force-feedback output.
/// Assist fractions the cars had before D-075, from their data files then.
const LEGACY_ASSIST: [(&str, f64); 2] = [("hatch_tern_gt", 0.0), ("sedan_halden_e4", 0.36)];

/// One sample of the steering comparison.
struct SteerRow {
    speed_kmh: f64,
    column_nm: f64,
    lateral_g: f64,
    gyro_nm: f64,
    friction_nm: f64,
}

/// Limits a scripted driver's steering to what hands on a wheel can do:
/// the path followers step the wheel instantly, which would read as an
/// impossible column acceleration through the rack inertia.
struct HandSteer {
    angle: f64,
    rate: f64,
    max_wheel_angle: f64,
}

/// Fastest steering-wheel rate of the comparison driver, rad/s.
const HAND_MAX_RATE: f64 = 12.0;
/// Largest steering-wheel acceleration of the comparison driver, rad/s².
const HAND_MAX_ACCEL: f64 = 300.0;

impl HandSteer {
    fn new(s: &Session) -> Self {
        Self {
            angle: 0.0,
            rate: 0.0,
            max_wheel_angle: s.car().params().steering.max_wheel_angle_rad,
        }
    }

    /// Moves toward the steer fraction `target` and returns the fraction.
    fn follow(&mut self, frame: &mut InputFrame, dt: f64) {
        let target =
            f64::from(frame.steer) / f64::from(qts_core::input::AXIS_MAX) * self.max_wheel_angle;
        let wanted = ((target - self.angle) / 0.04).clamp(-HAND_MAX_RATE, HAND_MAX_RATE);
        self.rate += (wanted - self.rate).clamp(-HAND_MAX_ACCEL * dt, HAND_MAX_ACCEL * dt);
        self.angle += self.rate * dt;
        let fraction = (self.angle / self.max_wheel_angle).clamp(-1.0, 1.0);
        frame.steer = (fraction * f64::from(qts_core::input::AXIS_MAX)).round() as i16;
    }
}

fn steering_session(
    root: &DataRoot,
    car: &str,
    track: &str,
    legacy: Option<f64>,
) -> Result<Session, String> {
    let config = SessionConfig {
        assists: Assists {
            auto_clutch: true,
            auto_shift: true,
            traction_control: true,
            engine_damage: false,
        },
        tire_start_temp_c: Some(60.0),
        ..SessionConfig::default()
    };
    let mut s = Session::from_data(root, car, track, config)?;
    s.car_mut().set_legacy_steering(legacy);
    Ok(s)
}

fn steer_row(s: &Session) -> SteerRow {
    let out = s.car().output();
    SteerRow {
        speed_kmh: out.speed * 3.6,
        column_nm: out.steering_torque_nm,
        lateral_g: out.accel_body[1] / 9.80665,
        gyro_nm: out.steering.gyro_nm,
        friction_nm: out.steering.friction_nm,
    }
}

fn cmd_tire_plots(args: &Args) -> Result<(), String> {
    use qts_sim::tire_plots::{curves, peak, svg};
    let root = data_root()?;
    let out = args.text("out").unwrap_or("docs/img");
    let surface = root
        .load_surface("tarmac")
        .map_err(|e| e.to_string())?
        .value;
    for id in root.list("tires").map_err(|e| e.to_string())? {
        let p = root.load_tire(&id).map_err(|e| e.to_string())?.value;
        let c = curves(&p, &surface);
        for (k, load) in c.loads.iter().enumerate() {
            let (a, fy) = peak(&c.lateral[k]);
            let (s, fx) = peak(&c.longitudinal[k]);
            println!(
                "{id:22} {load:6.0} N  lateral peak {:.3} x load at {a:5.2} deg  longitudinal peak {:.3} x load at slip {s:.3}",
                fy / load,
                fx / load
            );
        }
        let path = format!("{out}/tire_{id}.svg");
        std::fs::write(&path, svg(&c, &p.name)).map_err(|e| format!("{path}: {e}"))?;
        println!("  -> {path}");
    }
    Ok(())
}

fn cmd_lap_compare(args: &Args) -> Result<(), String> {
    use qts_sim::lap_telemetry::{compare, compare_svg, Lap};
    let [a, b] = args
        .positional
        .get(1..3)
        .ok_or("usage: lap-compare A.csv B.csv")?
    else {
        return Err("usage: lap-compare A.csv B.csv".into());
    };
    let a = Lap::read(a.as_ref())?;
    let b = Lap::read(b.as_ref())?;
    let step = args.number("step", 5.0)?.max(0.5);
    let c = compare(&a, &b, step)?;
    let time = |l: &Lap| {
        l.file
            .meta("lap_time_s")
            .filter(|t| !t.is_empty())
            .unwrap_or("not counted")
            .to_string()
    };
    println!("A {} ({})  {:.0} m", a.label, time(&a), a.length());
    println!("B {} ({})  {:.0} m", b.label, time(&b), b.length());
    let end = *c.delta.last().unwrap_or(&0.0);
    println!(
        "B minus A over {:.0} m: {end:+.3} s",
        c.distance.last().unwrap_or(&0.0)
    );
    // Where B loses and gains most, over 100 m stretches.
    let window = (100.0 / step).round().max(1.0) as usize;
    let mut stretches: Vec<(f64, f64)> = (0..c.delta.len().saturating_sub(window))
        .step_by(window)
        .map(|i| (c.distance[i], c.delta[i + window] - c.delta[i]))
        .collect();
    stretches.sort_by(|x, y| y.1.abs().total_cmp(&x.1.abs()));
    println!("Biggest differences over 100 m (positive: B loses time):");
    for (d, dt) in stretches.iter().take(6) {
        let i = (d / step) as usize;
        let speed = &c.channels[0];
        println!(
            "  {:5.0}-{:5.0} m  {dt:+.3} s   speed A {:5.1} B {:5.1} km/h",
            d,
            d + 100.0,
            speed.0[i],
            speed.1[i]
        );
    }
    if let Some(path) = args.text("svg") {
        std::fs::write(path, compare_svg(&a, &b, &c)).map_err(|e| format!("{path}: {e}"))?;
        println!("svg {path}");
    }
    Ok(())
}

fn cmd_ffb_gain(args: &Args) -> Result<(), String> {
    let root = data_root()?;
    let knee: f64 = args
        .text("knee")
        .map_or(Ok(0.75), |k| k.parse().map_err(|_| "bad --knee"))?;
    let wheels: Vec<String> = match args.text("wheel") {
        Some(w) => vec![w.to_string()],
        None => root.list("wheels").map_err(|e| e.to_string())?,
    };
    let mut worst: f64 = 0.0;
    println!("Column torque over a scripted Ridgeline lap (hand-limited steering), N.m:");
    for car in root.list("cars").map_err(|e| e.to_string())? {
        for (label, lateral) in [("0.7 g", 7.0), ("0.85 g", 8.5)] {
            let rows = steering_lap_at(&root, &car, None, lateral)?;
            let mut t: Vec<f64> = rows.iter().map(|r| r.column_nm.abs()).collect();
            t.sort_by(f64::total_cmp);
            let at = |p: f64| t[((t.len() - 1) as f64 * p).round() as usize];
            let p99 = at(0.99);
            worst = worst.max(p99);
            println!(
                "  {car:20} planned {label:6}  median {:5.1}  p95 {:5.1}  p99 {:5.1}  max {:5.1}",
                at(0.5),
                at(0.95),
                p99,
                t[t.len() - 1]
            );
        }
    }
    println!("Gain that puts the highest 99th percentile ({worst:.1} N.m) at a knee of {knee}:");
    for id in wheels {
        let w = root.load_wheel(&id).map_err(|e| e.to_string())?.value;
        let g = qts_ffb::suggest_gain(worst, knee, w.peak_torque_nm);
        println!(
            "  {:34} {:5.1} N.m{}  gain {:.2}",
            w.name,
            w.peak_torque_nm,
            if w.verified { "" } else { " (unverified)" },
            g
        );
    }
    Ok(())
}

/// A scripted lap of Ridgeline, sampled every 50 ms.
fn steering_lap(root: &DataRoot, car: &str, legacy: Option<f64>) -> Result<Vec<SteerRow>, String> {
    steering_lap_at(root, car, legacy, 7.0)
}

/// A scripted lap of Ridgeline planned for `lateral` m/s², every 50 ms.
fn steering_lap_at(
    root: &DataRoot,
    car: &str,
    legacy: Option<f64>,
    lateral: f64,
) -> Result<Vec<SteerRow>, String> {
    let mut s = steering_session(root, car, "ridgeline_circuit", legacy)?;
    let path = s.track().path("circuit").ok_or("no circuit path")?.clone();
    let mut driver = PathFollower::new(
        &path,
        PathFollowerConfig {
            max_speed: 45.0,
            lateral_accel: lateral,
            closed: true,
            ..PathFollowerConfig::default()
        },
    );
    let mut rows = Vec::new();
    let mut halfway = false;
    let mut hands = HandSteer::new(&s);
    for tick in 0..400_000u64 {
        let mut frame = driver.frame(&s);
        hands.follow(&mut frame, s.tick_dt());
        s.step(&frame);
        let d = driver.progress();
        halfway |= tick > 20_000 && d > 0.5 * path.length();
        if halfway && d < 0.1 * path.length() {
            break;
        }
        if tick % 50 == 0 {
            rows.push(steer_row(&s));
        }
    }
    Ok(rows)
}

/// The validation skidpad: a 30 m circle on the test track with the speed
/// raised slowly, one-second means while the car holds the line.
fn steering_skidpad(
    root: &DataRoot,
    car: &str,
    legacy: Option<f64>,
) -> Result<Vec<SteerRow>, String> {
    use qts_sim::track::MarkerDef;
    let mut s = steering_session(root, car, "test_track", legacy)?;
    let (center, radius) = s
        .track()
        .def()
        .markers
        .iter()
        .find_map(|m| match m {
            MarkerDef::Circle {
                center_x,
                center_y,
                radius_m,
                ..
            } => Some(([*center_x, *center_y], *radius_m)),
            _ => None,
        })
        .ok_or("no skidpad circle")?;
    let start = [center[0], center[1] - radius];
    s.car_mut().place(start[0], start[1], 0.0, 0.0);
    s.car_mut().set_forward_speed(6.0);
    let config = PathFollowerConfig {
        max_speed: 6.0,
        lateral_accel: 60.0,
        brake_decel: 6.0,
        lookahead_base: 2.5,
        lookahead_gain: 0.25,
        throttle_gain: 0.6,
        brake_gain: 0.3,
        closed: true,
    };
    let mut driver = PathFollower::circle(center, radius, start, config);
    let mut rows = Vec::new();
    let mut acc = [0.0; 6];
    let mut hands = HandSteer::new(&s);
    for tick in 0..300_000u64 {
        let target = 6.0 + 0.12 * tick as f64 / 1000.0;
        driver.set_max_speed(target);
        let mut frame = driver.frame(&s);
        hands.follow(&mut frame, s.tick_dt());
        s.step(&frame);
        let r = steer_row(&s);
        let pos = s.car().state.pos;
        let error = ((pos.x - center[0]).hypot(pos.y - center[1]) - radius).abs();
        for (a, v) in acc.iter_mut().zip([
            r.speed_kmh,
            r.column_nm,
            r.lateral_g,
            r.gyro_nm,
            r.friction_nm,
            error,
        ]) {
            *a += v;
        }
        if tick % 1000 == 999 {
            let m: Vec<f64> = acc.iter().map(|a| a / 1000.0).collect();
            acc = [0.0; 6];
            if m[5] > 4.0 || target - m[0] / 3.6 > 4.0 {
                break;
            }
            if m[5] < 1.5 && tick > 3000 {
                rows.push(SteerRow {
                    speed_kmh: m[0],
                    column_nm: m[1],
                    lateral_g: m[2],
                    gyro_nm: m[3],
                    friction_nm: m[4],
                });
            }
        }
    }
    Ok(rows)
}

fn cmd_steering_compare(args: &Args) -> Result<(), String> {
    let root = data_root()?;
    let mut csv = String::from("car,model,run,speed_kmh,column_nm,lateral_g,gyro_nm,friction_nm\n");
    for (car, fraction) in LEGACY_ASSIST {
        println!("{car}");
        for (model, legacy) in [("before", Some(fraction)), ("now", None)] {
            for run in ["ridgeline", "skidpad"] {
                let rows = if run == "ridgeline" {
                    steering_lap(&root, car, legacy)?
                } else {
                    steering_skidpad(&root, car, legacy)?
                };
                for r in &rows {
                    csv.push_str(&format!(
                        "{car},{model},{run},{:.2},{:.4},{:.4},{:.4},{:.4}\n",
                        r.speed_kmh, r.column_nm, r.lateral_g, r.gyro_nm, r.friction_nm
                    ));
                }
                // Column torque per g of lateral acceleration, by speed band.
                let mut line = format!("  {model:6} {run:9}");
                for (lo, hi) in [(0.0, 40.0), (40.0, 80.0), (80.0, 120.0), (120.0, 250.0)] {
                    let band: Vec<&SteerRow> = rows
                        .iter()
                        .filter(|r| {
                            r.speed_kmh >= lo && r.speed_kmh < hi && r.lateral_g.abs() > 0.2
                        })
                        .collect();
                    if band.is_empty() {
                        line.push_str(&format!("  {lo:>3.0}-{hi:<3.0} km/h      -    "));
                        continue;
                    }
                    let per_g = band.iter().map(|r| r.column_nm / r.lateral_g).sum::<f64>()
                        / band.len() as f64;
                    line.push_str(&format!("  {lo:>3.0}-{hi:<3.0} km/h {per_g:5.1} N.m/g"));
                }
                println!("{line}");
            }
        }
    }
    if let Some(path) = args.text("csv") {
        std::fs::write(path, csv).map_err(|e| format!("{path}: {e}"))?;
        println!("csv {path}");
    }
    Ok(())
}

fn cmd_kerb_ffb(args: &Args) -> Result<(), String> {
    use qts_sim::drivers::{PathFollower, PathFollowerConfig};
    let root = data_root()?;
    let car = args.text("car").unwrap_or("hatch_tern_gt");
    let preset = args.text("ffb").unwrap_or("fanatec_csl_elite");
    let ffb = root.load_ffb(preset).map_err(|e| e.to_string())?.value;
    let config = SessionConfig {
        assists: Assists {
            auto_clutch: true,
            auto_shift: true,
            traction_control: true,
            engine_damage: false,
        },
        ..SessionConfig::default()
    };
    // (distance, on kerb, column torque, output, detail, front-right load)
    let mut runs: Vec<Vec<[f64; 6]>> = Vec::new();
    for tilt in [false, true] {
        let mut assets =
            SessionAssets::load(&root, car, "ridgeline_circuit").map_err(|e| e.to_string())?;
        assets.track.set_legacy_kerbs(!tilt);
        let mut s = Session::new(assets, config)?;
        let mut pipeline = FfbPipeline::new(ffb.clone(), s.tick_rate_hz())?;
        let path = s.track().path("circuit").ok_or("no circuit path")?.clone();
        let kerb = s
            .track()
            .surfaces()
            .iter()
            .position(|x| x.id == "kerb")
            .unwrap_or(usize::MAX);
        let mut driver = PathFollower::offset(
            &path,
            -5.6,
            PathFollowerConfig {
                max_speed: 18.0,
                ..PathFollowerConfig::default()
            },
        );
        let mut rows = Vec::new();
        for _ in 0..80_000 {
            let frame = driver.frame(&s);
            s.step(&frame);
            let sample = pipeline.step(&ffb_input(&s, None));
            let out = s.car().output();
            let d = driver.progress();
            if (440.0..=560.0).contains(&d) {
                rows.push([
                    d,
                    if usize::from(out.wheels[1].surface_index) == kerb {
                        1.0
                    } else {
                        0.0
                    },
                    out.steering_torque_nm,
                    sample.output,
                    sample.detail_nm,
                    out.wheels[1].load,
                ]);
            }
            if d > 560.0 {
                break;
            }
        }
        runs.push(rows);
    }
    let stats = |rows: &[[f64; 6]]| {
        let on: Vec<&[f64; 6]> = rows.iter().filter(|r| r[1] > 0.0).collect();
        let entry = rows.iter().position(|r| r[1] > 0.0).unwrap_or(0);
        let before = rows[entry.saturating_sub(200)..entry]
            .iter()
            .map(|r| r[2])
            .sum::<f64>()
            / (entry - entry.saturating_sub(200)).max(1) as f64;
        let mean_on = on.iter().map(|r| r[2]).sum::<f64>() / on.len().max(1) as f64;
        // Peak change of column torque over 50 ms around climbing on.
        let window = &rows[entry.saturating_sub(50)..(entry + 150).min(rows.len())];
        let (lo, hi) = window.iter().fold((f64::MAX, f64::MIN), |(lo, hi), r| {
            (lo.min(r[2]), hi.max(r[2]))
        });
        let out_rms = (on.iter().map(|r| r[3] * r[3]).sum::<f64>() / on.len().max(1) as f64).sqrt();
        (on.len(), before, mean_on, hi - lo, out_rms)
    };
    println!("{car} on Ridgeline T1 inside kerb (right wheels), preset {preset}");
    println!("                            ticks on kerb  torque before  torque on kerb  swing entering  output RMS on kerb");
    for (name, rows) in ["before: flat, square ends", "now: tilted, tapered"]
        .iter()
        .zip(&runs)
    {
        let (n, before, on, swing, rms) = stats(rows);
        println!(
            "  {name:26} {n:13}  {before:+10.2} N.m  {on:+11.2} N.m  {swing:11.2} N.m  {rms:17.3}"
        );
    }
    if let Some(path) = args.text("csv") {
        let mut text = String::from("tick,distance_m,flat_on_kerb,flat_column_nm,flat_output,tilt_on_kerb,tilt_column_nm,tilt_output\n");
        for (i, (a, b)) in runs[0].iter().zip(&runs[1]).enumerate() {
            text.push_str(&format!(
                "{i},{:.3},{},{:.4},{:.4},{},{:.4},{:.4}\n",
                a[0], a[1], a[2], a[3], b[1], b[2], b[3]
            ));
        }
        std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))?;
        println!("csv        {path}");
    }
    Ok(())
}

fn cmd_ffb_report(args: &Args) -> Result<(), String> {
    let files = &args.positional[1..];
    if files.is_empty() {
        return Err("ffb-report needs one or more log files".into());
    }
    for file in files {
        let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
        println!("== {file}");
        println!("{}", qts_sim::ffb_log::report(&text)?);
    }
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
        "bench" => cmd_bench(&args).map(|()| true),
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
        "ffb-report" => cmd_ffb_report(&args).map(|()| true),
        "kerb-ffb" => cmd_kerb_ffb(&args).map(|()| true),
        "steering-compare" => cmd_steering_compare(&args).map(|()| true),
        "ffb-gain" => cmd_ffb_gain(&args).map(|()| true),
        "lap-compare" => cmd_lap_compare(&args).map(|()| true),
        "coach" => cmd_coach(&args).map(|()| true),
        "ai" => cmd_ai(&args).map(|()| true),
        "cold-start" => cmd_cold_start(&args).map(|()| true),
        "setup-sheet" => cmd_setup_sheet(&args).map(|()| true),
        "tire-plots" => cmd_tire_plots(&args).map(|()| true),
        "ffb-devices" => cmd_ffb_devices().map(|()| true),
        "input-read" => cmd_input_read(&args).map(|()| true),
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
