//! Zero cost stack overflow protection

use std::env;
use std::process::Command;

const DEFAULT_LD: &'static str = "riscv64-unknown-elf-ld";
const DEFAULT_SIZE: &'static str = "riscv64-unknown-elf-size";
const ENV_LD: &'static str = "CKB_STD_LD";

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();

    // run the linker exactly as `rustc` instructed
    let ld_cmd = env::var(ENV_LD).unwrap_or_else(|_| DEFAULT_LD.to_string());
    let mut ld1 = Command::new(&ld_cmd);
    ld1.args(&args);
    eprintln!("{:?}", ld1);
    assert!(ld1.status().unwrap().success());

    // retrieve the output file name
    let mut output = None;
    let mut iargs = args.iter();
    while let Some(arg) = iargs.next() {
        if arg == "-o" {
            output = iargs.next();
            break;
        }
    }

    let output = output.unwrap();

    // shell out to `size` to get the size of the linker sections
    // TODO use a library instead of calling `size` (?)
    let mut size = Command::new(DEFAULT_SIZE);
    size.arg("-A").arg(output);
    eprintln!("{:?}", size);
    let stdout = String::from_utf8(size.output().unwrap().stdout).unwrap();

    // parse the stdout of `size`
    let mut bss = None;
    let mut data = None;
    let mut heap = None;
    let mut sram = None;
    let mut ram = None;
    for line in stdout.lines() {
        if line.starts_with(".bss") {
            // e.g. .bss $bss 0x20000000
            bss = line
                .split_whitespace()
                .nth(1)
                .map(|s| s.parse::<u32>().expect(".bss size should've be an integer"));
        } else if line.starts_with(".data") {
            // e.g. .data $data 0x20000010
            data = line.split_whitespace().nth(1).map(|s| {
                s.parse::<u32>()
                    .expect(".data size should've be an integer")
            });
        } else if line.starts_with(".stack") {
            // e.g. .stack $ram $sram
            let mut parts = line.split_whitespace().skip(1);
            ram = parts.next().map(|s| {
                s.parse::<u32>()
                    .expect(".stack size should've been an integer")
            });
            sram = parts.next().map(|s| {
                s.parse::<u32>()
                    .expect(".stack addr should've been an integer")
            });
        } else if line.starts_with(".heap") {
            // e.g. .heap $heap 0x20000020
            heap = line.split_whitespace().nth(1).map(|s| {
                s.parse::<u32>()
                    .expect(".heap size should've been an integer")
            });
        }
    }

    // compute the new start address of the (.bss+.data) section
    // the relocated stack will start at that address as well (and grow downwards)
    let bss = bss.unwrap_or(0);
    let data = data.unwrap_or(0);
    let heap = heap.unwrap_or(0);
    let sram = sram.expect(".stack section missing.");
    let ram = ram.expect(".stack section missing.");
    let eram = sram + ram;

    let sbss = eram - bss - data - heap;

    let mut ld2 = Command::new(&ld_cmd);
    ld2.arg(format!("--defsym=_sbss={}", sbss))
        .arg(format!("--defsym=_stack_start={}", sbss))
        .args(&args);
    eprintln!("{:?}", ld2);
    assert!(ld2.status().unwrap().success());
}
