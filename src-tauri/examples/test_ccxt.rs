use hidapi::HidApi;
use std::thread::sleep;
use std::time::Duration;

const BUF_W: usize = 385;
const BUF_R: usize = 384;

// Protocol constants from OpenLinkHub ccxt.go
const CMD_CLOSE_ENDPOINT: &[u8] = &[0x05, 0x01, 0x01];
const CMD_OPEN_ENDPOINT: &[u8] = &[0x0d, 0x01];
const CMD_READ: &[u8] = &[0x08, 0x01];

fn main() {
    let api = HidApi::new().expect("HidApi");
    let serial = "230620119ac184aa6692ea062091005f";

    println!("=== CCXT protocol dump (4-step endpoint) ===\n");
    let dev = api.open_serial(0x1b1c, 0x0c2a, serial).expect("open");
    dev.set_blocking_mode(false).ok();

    // 1) Software mode
    println!("[1] CMD_SOFTWARE_MODE");
    let r = xfer(&dev, &[0x01, 0x03, 0x00, 0x02], None);
    dump("SW_MODE", &r);
    sleep(Duration::from_millis(50));

    // 2) Firmware
    println!("\n[2] CMD_GET_FIRMWARE");
    let r = xfer(&dev, &[0x02, 0x13], None);
    dump("FW", &r);
    if r.len() >= 7 {
        let v3 = u16::from_le_bytes([r[5], r[6]]);
        println!("  → Firmware: {}.{}.{}", r[3], r[4], v3);
    }

    // 3) Read fans (full 4-step: close→open→read→close)
    println!("\n[3] read_endpoint(GET_FANS=0x1a)");
    let r = read_endpoint(&dev, &[0x1a]);
    dump("FANS", &r);
    println!(
        "  → channel_count = resp[5] = {}",
        r.get(5).copied().unwrap_or(0)
    );
    if r.len() > 6 {
        let count = r[5] as usize;
        let data = &r[6..];
        for i in 0..count.min(data.len()) {
            let status = data[i];
            println!("    fan[{i}] status=0x{status:02x} (0x07=connected)");
        }
    }

    // 4) Read speeds
    println!("\n[4] read_endpoint(GET_SPEEDS=0x17)");
    let r = read_endpoint(&dev, &[0x17]);
    dump("SPEEDS", &r);
    if r.len() > 6 {
        let data = &r[6..];
        for i in 0..6 {
            let s = i * 2;
            if s + 2 <= data.len() {
                let rpm = i16::from_le_bytes([data[s], data[s + 1]]);
                println!("    speed[{i}] = {rpm} RPM");
            }
        }
    }

    // 5) Read temps (3 bytes per channel: status, temp_lo, temp_hi)
    println!("\n[5] read_endpoint(GET_TEMPS=0x21)");
    let r = read_endpoint(&dev, &[0x21]);
    dump("TEMPS", &r);
    println!(
        "  → channel_count = resp[5] = {}",
        r.get(5).copied().unwrap_or(0)
    );
    if r.len() > 6 {
        let count = r[5] as usize;
        let data = &r[6..];
        for i in 0..count {
            let s = i * 3;
            if s + 3 <= data.len() {
                let status = data[s];
                let temp_raw = u16::from_le_bytes([data[s + 1], data[s + 2]]);
                let temp = temp_raw as f32 / 10.0;
                println!("    temp[{i}] status=0x{status:02x} raw={temp_raw} = {temp:.1}°C");
            }
        }
    }

    // 6) Hardware mode (restore)
    println!("\n[6] CMD_HARDWARE_MODE (restore)");
    xfer(&dev, &[0x01, 0x03, 0x00, 0x01], None);
    println!("  Done.");
}

/// Full 4-step endpoint read: close→open→read→close
fn read_endpoint(dev: &hidapi::HidDevice, mode: &[u8]) -> Vec<u8> {
    xfer(dev, CMD_CLOSE_ENDPOINT, Some(mode));
    xfer(dev, CMD_OPEN_ENDPOINT, Some(mode));
    let data = xfer(dev, CMD_READ, Some(mode));
    xfer(dev, CMD_CLOSE_ENDPOINT, Some(mode));
    data
}

fn xfer(dev: &hidapi::HidDevice, cmd: &[u8], data: Option<&[u8]>) -> Vec<u8> {
    let mut buf = vec![0u8; BUF_W];
    buf[0] = 0x00;
    buf[1] = 0x08;
    buf[2..2 + cmd.len()].copy_from_slice(cmd);
    if let Some(d) = data {
        let off = 2 + cmd.len();
        buf[off..off + d.len()].copy_from_slice(d);
    }

    if let Err(e) = dev.write(&buf) {
        println!("  TX ERROR: {e}");
        return vec![];
    }

    let mut rbuf = vec![0u8; BUF_R];
    for _ in 0..40 {
        match dev.read_timeout(&mut rbuf, 10) {
            Ok(0) => continue,
            Ok(n) => {
                rbuf.truncate(n);
                return rbuf;
            }
            Err(e) => {
                println!("  RX ERROR: {e}");
                return vec![];
            }
        }
    }
    println!("  RX timeout");
    vec![]
}

fn dump(label: &str, data: &[u8]) {
    let show = data.len().min(40);
    println!(
        "  {label} len={} first {show}: {:02x?}",
        data.len(),
        &data[..show]
    );
}
