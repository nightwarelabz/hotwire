//! The `hotwire` CLI: `convert` and `watch` (see SPEC.md).

use hotwire::config::{AssetClass, Config};
use hotwire::convert::convert_file;
use std::path::PathBuf;

const USAGE: &str = "\
hotwire — the C35 x Blender bridge

USAGE:
  hotwire convert <input.glb> <out_dir> [--config hotwire.toml] [--class prop|character]
  hotwire watch   <drop_dir> <models_dir> [--config hotwire.toml] [--class prop|character]

convert  Convert one .glb to .c35m model(s) + manifest lines.
         Refusals (skinned input, over era budget) write nothing
         and exit non-zero (2 = over budget, 3 = skinned).
watch    Poll the drop folder (1s) and convert new/changed .glb
         files into the models folder. Never stops on a bad file.
";

struct Args {
    positional: Vec<PathBuf>,
    config: Option<PathBuf>,
    class: AssetClass,
}

fn parse_args(rest: &[String]) -> Result<Args, String> {
    let mut out = Args {
        positional: Vec::new(),
        config: None,
        class: AssetClass::Prop,
    };
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--config" => {
                i += 1;
                let v = rest
                    .get(i)
                    .ok_or_else(|| "--config wants a path".to_string())?;
                out.config = Some(PathBuf::from(v));
            }
            "--class" => {
                i += 1;
                let v = rest
                    .get(i)
                    .ok_or_else(|| "--class wants prop|character".to_string())?;
                out.class = AssetClass::parse(v)?;
            }
            flag if flag.starts_with("--") => {
                return Err(format!("unknown flag '{flag}'"));
            }
            value => out.positional.push(PathBuf::from(value)),
        }
        i += 1;
    }
    Ok(out)
}

fn load_config(args: &Args) -> Result<Config, String> {
    Config::discover(args.config.as_deref())
}

fn cmd_convert(args: &Args) -> i32 {
    if args.positional.len() != 2 {
        eprintln!("hotwire convert wants <input.glb> <out_dir>\n\n{USAGE}");
        return 1;
    }
    let cfg = match load_config(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("hotwire: {e}");
            return 1;
        }
    };
    let (input, out_dir) = (&args.positional[0], &args.positional[1]);
    match convert_file(input, out_dir, &cfg, args.class) {
        Ok(report) => {
            report.print();
            0
        }
        Err(e) => {
            // Refusals are reports: print them where the
            // conversion reports go, exit with the code.
            println!("{}", e.describe());
            e.exit_code()
        }
    }
}

fn cmd_watch(args: &Args) -> i32 {
    if args.positional.len() != 2 {
        eprintln!("hotwire watch wants <drop_dir> <models_dir>\n\n{USAGE}");
        return 1;
    }
    let cfg = match load_config(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("hotwire: {e}");
            return 1;
        }
    };
    hotwire::watch::run(&args.positional[0], &args.positional[1], &cfg, args.class)
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let code = match argv.first().map(|s| s.as_str()) {
        Some("convert") => match parse_args(&argv[1..]) {
            Ok(a) => cmd_convert(&a),
            Err(e) => {
                eprintln!("hotwire: {e}\n\n{USAGE}");
                1
            }
        },
        Some("watch") => match parse_args(&argv[1..]) {
            Ok(a) => cmd_watch(&a),
            Err(e) => {
                eprintln!("hotwire: {e}\n\n{USAGE}");
                1
            }
        },
        _ => {
            eprintln!("{USAGE}");
            1
        }
    };
    std::process::exit(code);
}
