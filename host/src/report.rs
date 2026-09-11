//! Console + CSV reporting for campaign results.

use std::io::Write;

use crate::campaign::{CampaignResult, FAULT_LABELS};

pub fn print(res: &CampaignResult) {
    println!();
    println!("=== FAULT INJECTION CAMPAIGN REPORT ===");
    println!(
        "seed={}  profile={}  mode={}  packets={}",
        res.seed,
        res.profile,
        if res.simulated { "simulated" } else { "hardware" },
        res.packets
    );
    println!(
        "injected faults: {}   detected by DUT: {}   missed: {}   physical(out-of-band): {}",
        res.injected, res.detected, res.missed, res.physical
    );

    if res.injected > 0 {
        let rate = (res.detected as f64 / res.injected as f64) * 100.0;
        println!("detection rate: {rate:.1}%");
    }
    if res.detected > 0 {
        println!(
            "detection latency: avg {:.2} ms, max {} ms",
            res.avg_latency_ms, res.max_latency_ms
        );
    }

    println!();
    println!("  {:<10} {:>8} {:>8}", "fault kind", "injected", "detected");
    for k in &res.per_kind {
        let label = FAULT_LABELS.get(k.kind as usize).copied().unwrap_or("?");
        println!("  {label:<10} {:>8} {:>8}", k.injected, k.detected);
    }

    let csv = to_csv(res);
    let path = format!("faultforge_out/campaign_{}_{}.csv", res.seed, res.profile);
    std::fs::create_dir_all("faultforge_out").ok();
    let mut f = match std::fs::File::create(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("could not write {path}: {e}");
            return;
        }
    };
    let _ = f.write_all(csv.as_bytes());
    println!();
    println!("report written to {path}");
}

pub fn to_csv(res: &CampaignResult) -> String {
    let mut s = String::from("fault_id,fault_kind,at_seq,detected,detection,latency_ms\n");
    for (fault_id, kind, at_seq, detected, label, lat) in &res.rows {
        s.push_str(&format!(
            "{fault_id},{kind},{at_seq},{detected},{label},{lat}\n"
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::campaign::{CampaignResult, KindStat};

    #[test]
    fn csv_has_header_and_rows() {
        let res = CampaignResult {
            seed: 1,
            profile: "fuzz",
            packets: 10,
            simulated: true,
            injected: 2,
            detected: 1,
            missed: 1,
            physical: 0,
            avg_latency_ms: 1.5,
            max_latency_ms: 2,
            per_kind: vec![KindStat { kind: 3, injected: 1, detected: 1 }],
            rows: vec![
                (1, 3, 10, true, "gap", 1),
                (2, 4, 22, false, "__miss__", 0),
            ],
        };
        let csv = to_csv(&res);
        assert!(csv.starts_with("fault_id,fault_kind,at_seq,detected,detection,latency_ms\n"));
        assert!(csv.contains("1,3,10,true,gap,1\n"));
        assert!(csv.contains("2,4,22,false,__miss__,0\n"));
    }
}