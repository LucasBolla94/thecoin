//! `tccl` — TCCL developer tool: check, inspect and simulate contracts locally.

use std::path::PathBuf;
use std::process::ExitCode;
use tccl::abi::{display, parse_arg};
use tccl::program::{FnKind, Value};
use tccl::sim::{account, Simulator};
use tccl::{compile, CompileOptions};

const USAGE: &str = "\
tccl — The Coin Cloud Language tool

USAGE:
  tccl check <file.tccl>                 Compile and show the contract interface
  tccl abi <file.tccl>                   Print the interface as JSON
  tccl run <file.tccl> [options] deploy [args...]
  tccl run <file.tccl> [options] call <function> [args...]
  tccl run <file.tccl> [options] view <function> [args...]
  tccl ring keygen [--seed <hex32>]      Create a ring (privacy) key pair
  tccl ring sign --secret <hex> --ring <pk1,pk2,...> --index <i> --message <0xhex>

RUN OPTIONS:
  --state <file>     Simulator state file (default: tccl-state.json)
  --from <name>      Test account calling (default: alice); every account starts with 1 000 000 TCN
  --value <amount>   TCN sent with the call, e.g. 5tcn or 500000000 (motes)
  --height <n>       Set the simulated block height before the call
  --contract <hex>   Contract address in the simulator (default: the last deployed)

Arguments are parsed using the declared parameter types:
  int 42 | 2.5tcn    bool true    text \"hello\"    bytes 0xabcd    address tc1...    list [1, 2]
Account names are accepted for address parameters: @alice, @bob ...
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn read_source(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))
}

fn take_opt(args: &mut Vec<String>, name: &str) -> Option<String> {
    let pos = args.iter().position(|a| a == name)?;
    if pos + 1 >= args.len() {
        return None;
    }
    let v = args.remove(pos + 1);
    args.remove(pos);
    Some(v)
}

fn run(mut args: Vec<String>) -> Result<(), String> {
    if args.is_empty() || args[0] == "help" || args[0] == "--help" || args[0] == "-h" {
        print!("{USAGE}");
        return Ok(());
    }
    let cmd = args.remove(0);
    match cmd.as_str() {
        "--version" | "version" => {
            println!("tccl {} (language version {})", env!("CARGO_PKG_VERSION"), tccl::program::LANGUAGE_VERSION);
            Ok(())
        }
        "check" => {
            let path = args.first().ok_or("usage: tccl check <file.tccl>")?;
            let src = read_source(path)?;
            let p = compile(&src, &CompileOptions::default()).map_err(|e| format!("{path}:{e}"))?;
            println!("✔ {} compiles ({} bytes of source, {} bytes compiled)", p.name, src.len(), p.to_bytes().len());
            println!(
                "  state: {}",
                if p.states.is_empty() {
                    "-".into()
                } else {
                    p.states.iter().map(|s| format!("{}: {}", s.name, s.ty)).collect::<Vec<_>>().join(", ")
                }
            );
            for f in p.abi() {
                let params = f.params.iter().map(|(n, t)| format!("{n}: {t}")).collect::<Vec<_>>().join(", ");
                let kind = match f.kind {
                    FnKind::Init => "init",
                    FnKind::Action => "action",
                    FnKind::View => "view",
                    FnKind::Internal => "fn",
                };
                let ret = if f.returns == "nothing" { String::new() } else { format!(" -> {}", f.returns) };
                println!("  {kind} {}({params}){ret}{}", f.name, if f.payable { " payable" } else { "" });
            }
            Ok(())
        }
        "abi" => {
            let path = args.first().ok_or("usage: tccl abi <file.tccl>")?;
            let p = compile(&read_source(path)?, &CompileOptions::default()).map_err(|e| format!("{path}:{e}"))?;
            println!("{}", serde_json::to_string_pretty(&p.abi()).expect("json"));
            Ok(())
        }
        "run" => run_sim(args),
        "ring" => ring_cmd(args),
        other => Err(format!("unknown command '{other}'\n\n{USAGE}")),
    }
}

fn parse_call_args(program: &tccl::Program, function: &str, raw: &[String]) -> Result<Vec<Value>, String> {
    let (_, f) = program.find(function).ok_or_else(|| format!("contract has no function '{function}'"))?;
    if raw.len() != f.params.len() {
        return Err(format!(
            "{function} expects {} argument(s): {}",
            f.params.len(),
            f.params.iter().map(|(n, t)| format!("{n}: {t}")).collect::<Vec<_>>().join(", ")
        ));
    }
    f.params
        .iter()
        .zip(raw)
        .map(|((name, t), a)| {
            if let (tccl::Type::Address, Some(n)) = (t, a.strip_prefix('@')) {
                return Ok(Value::Address(account(n)));
            }
            parse_arg(a, t).map_err(|e| format!("argument '{name}': {e}"))
        })
        .collect()
}

fn run_sim(mut args: Vec<String>) -> Result<(), String> {
    let state_path = PathBuf::from(take_opt(&mut args, "--state").unwrap_or_else(|| "tccl-state.json".into()));
    let from = take_opt(&mut args, "--from").unwrap_or_else(|| "alice".into());
    let value = match take_opt(&mut args, "--value") {
        Some(v) => match parse_arg(&v, &tccl::Type::Int)? {
            Value::Int(i) if i >= 0 && i <= u64::MAX as i128 => i as u64,
            _ => return Err("invalid --value".into()),
        },
        None => 0,
    };
    let height = take_opt(&mut args, "--height");
    let contract_opt = take_opt(&mut args, "--contract");
    if args.len() < 2 {
        return Err("usage: tccl run <file.tccl> deploy|call|view ...".into());
    }
    let file = args.remove(0);
    let action = args.remove(0);
    let mut sim = match std::fs::read_to_string(&state_path) {
        Ok(json) => Simulator::load(&json)?,
        Err(_) => Simulator::new(),
    };
    if let Some(h) = height {
        sim.height = h.parse().map_err(|_| "invalid --height")?;
    }
    let caller = account(&from);
    let hrp = "tcr";
    let last_path = state_path.with_extension("last");
    let result = match action.as_str() {
        "deploy" => {
            let src = read_source(&file)?;
            let program = compile(&src, &CompileOptions::default()).map_err(|e| format!("{file}:{e}"))?;
            let call_args = if program.find("init").is_some() { parse_call_args(&program, "init", &args)? } else { Vec::new() };
            let (addr, r) = sim.deploy(&src, caller, call_args, value)?;
            if r.result.is_ok() {
                println!("deployed {} at {addr}", program.name);
                std::fs::write(&last_path, &addr).map_err(|e| e.to_string())?;
            }
            r
        }
        "call" | "view" => {
            if args.is_empty() {
                return Err(format!("usage: tccl run <file> {action} <function> [args...]"));
            }
            let function = args.remove(0);
            let addr = match contract_opt {
                Some(a) => a,
                None => std::fs::read_to_string(&last_path).map_err(|_| "no deployed contract yet (run deploy first)")?,
            };
            let program = sim.program(&addr).ok_or_else(|| format!("no contract at {addr}"))?.clone();
            let call_args = parse_call_args(&program, &function, &args)?;
            if action == "call" {
                sim.call(&addr, caller, &function, call_args, value)?
            } else {
                sim.view(&addr, &function, call_args)?
            }
        }
        other => return Err(format!("unknown run action '{other}' (use deploy, call or view)")),
    };
    for e in &r_events(&result) {
        println!("event {}({})", e.name, e.fields.iter().map(|(n, v)| format!("{n}: {}", display(v, hrp))).collect::<Vec<_>>().join(", "));
    }
    match &result.result {
        Ok(v) => {
            if *v != Value::Unit {
                println!("result: {}", display(v, hrp));
            }
            println!("ok · fuel used: {} · height: {} · {from} balance: {} motes", result.fuel_used, sim.height, sim.balance_of(&caller));
        }
        Err(e) => println!("FAILED: {e} · fuel used: {} (all changes reverted)", result.fuel_used),
    }
    std::fs::write(&state_path, sim.save()).map_err(|e| format!("cannot write {}: {e}", state_path.display()))?;
    if result.result.is_err() {
        return Err("call failed".into());
    }
    Ok(())
}

fn r_events(r: &tccl::sim::CallResult) -> Vec<tccl::sim::Event> {
    r.events.clone()
}

fn hex32(s: &str, what: &str) -> Result<[u8; 32], String> {
    let b = hex::decode(s.trim().trim_start_matches("0x")).map_err(|_| format!("{what} must be hex"))?;
    b.try_into().map_err(|_| format!("{what} must be 32 bytes"))
}

fn ring_cmd(mut args: Vec<String>) -> Result<(), String> {
    if args.is_empty() {
        return Err("usage: tccl ring keygen | tccl ring sign ...".into());
    }
    let sub = args.remove(0);
    match sub.as_str() {
        "keygen" => {
            let seed = match take_opt(&mut args, "--seed") {
                Some(s) => hex32(&s, "seed")?,
                None => {
                    let mut s = [0u8; 32];
                    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut s);
                    s
                }
            };
            let (secret, public) = tccl::ring::keypair_from_seed(&seed);
            println!("secret:    {}", hex::encode(secret));
            println!("public:    0x{}", hex::encode(public));
            println!("key image: 0x{}", hex::encode(tccl::ring::key_image(&secret).expect("valid")));
            Ok(())
        }
        "sign" => {
            let secret = hex32(&take_opt(&mut args, "--secret").ok_or("--secret required")?, "secret")?;
            let ring: Vec<[u8; 32]> = take_opt(&mut args, "--ring")
                .ok_or("--ring required")?
                .split(',')
                .map(|k| hex32(k, "ring key"))
                .collect::<Result<_, _>>()?;
            let index: usize = take_opt(&mut args, "--index").ok_or("--index required")?.parse().map_err(|_| "invalid --index")?;
            let msg_hex = take_opt(&mut args, "--message").ok_or("--message required")?;
            let msg = hex::decode(msg_hex.trim_start_matches("0x")).map_err(|_| "--message must be hex")?;
            let (sig, ki) = tccl::ring::sign(&msg, &ring, index, &secret, &mut rand::rngs::OsRng).map_err(|e| e.to_string())?;
            println!("signature: 0x{}", hex::encode(sig));
            println!("key image: 0x{}", hex::encode(ki));
            Ok(())
        }
        other => Err(format!("unknown ring command '{other}'")),
    }
}
