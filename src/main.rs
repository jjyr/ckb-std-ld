//! Zero cost stack overflow protection

use std::env;
use std::process::Command;

const ALIGN: u32 = 8;
const PAGE_ALIGN: u32 = 4096;

const DEFAULT_LD: &'static str = "rust-lld";
const DEFAULT_LD_ARGS: [&'static str; 4] = [
    "-flavor",
    "ld.lld",
    "-zseparate-code",
    "-zseparate-loadable-segments",
];
const DEFAULT_SIZE: &'static str = "riscv64-unknown-elf-size";

fn roundup(n: u32, align: u32) -> u32 {
    ((n - 1) / align + 1) * align
}

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();

    // run the linker exactly as `rustc` instructed
    let ld_cmd = DEFAULT_LD;
    let mut ld1 = Command::new(&ld_cmd);
    ld1.args(&DEFAULT_LD_ARGS);
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
    let mut text = None;
    let mut bss = None;
    let mut data = None;
    let mut heap = None;
    let mut sram = None;
    let mut ram = None;
    let mut others_size = Vec::new();
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
        } else if line.starts_with(".text") {
            // e.g. .text $text
            text = line.split_whitespace().nth(1).map(|s| {
                s.parse::<u32>()
                    .expect(".text size should've been an integer")
            });
        } else if line.starts_with(".heap") {
            // e.g. .heap $heap
            heap = line.split_whitespace().nth(1).map(|s| {
                s.parse::<u32>()
                    .expect(".heap size should've been an integer")
            });
        } else if line.starts_with(".") {
            let size = line
                .split_whitespace()
                .nth(1)
                .map(|s| {
                    s.parse::<u32>()
                        .expect(".heap size should've been an integer")
                })
                .unwrap_or_default();
            others_size.push(size);
        }
    }

    // compute the new start address of the (.bss+.data) section
    // the relocated stack will start at that address as well (and grow downwards)
    let text = text.expect(".text section missing.");
    let bss = bss.unwrap_or(0);
    let data = data.unwrap_or(0);
    let heap = heap.unwrap_or(0);
    let sram = sram.expect(".stack section missing.");
    let ram = ram.expect(".stack section missing.");
    let eram = sram + ram;

    // since CKB-VM W^X permission is page based,
    // text must be aligned to a single page.
    let sstack = eram
        - roundup(text, PAGE_ALIGN)
        - roundup(bss, ALIGN)
        - roundup(data, ALIGN)
        - roundup(heap, ALIGN)
        - others_size
            .into_iter()
            .map(|s| roundup(s, ALIGN))
            .sum::<u32>();

    // round down stack size to page
    let sstack = sstack / PAGE_ALIGN * PAGE_ALIGN;

    let mut ld2 = Command::new(&ld_cmd);
    ld2.args(&DEFAULT_LD_ARGS);
    ld2.arg(format!("--defsym=_stack_start={}", sstack))
        .args(&args);
    eprintln!("{:?}", ld2);
    assert!(ld2.status().unwrap().success());
}
