//! Statement-order rewriter shared by reader and CLI acceptance tests.
use haecceity::{
    p21::streaming::{self, Event},
    step::manifest::{self, ScanOptions},
};
use std::io::Cursor;

pub fn layouts(b: &[u8]) -> Vec<Vec<u8>> {
    let m = manifest::scan(Cursor::new(b), &ScanOptions::default()).unwrap();
    let mut rows = Vec::new();
    streaming::scan(b, 4096, |event| {
        if let Event::Instance(i, _) = event {
            rows.push((i.offset as usize, i.length as usize));
        }
        Ok(())
    })
    .unwrap();
    let first = rows.first().unwrap().0;
    let last = rows.last().unwrap();
    let end = last.0 + last.1;
    let mut buckets = vec![Vec::new(); m.products.len() + 1];
    for (i, &(offset, _)) in rows.iter().enumerate() {
        let owner = m
            .products
            .iter()
            .position(|p| {
                p.ranges
                    .iter()
                    .any(|r| r[0] <= offset as u64 && (offset as u64) < r[1])
            })
            .map_or(0, |p| p + 1);
        buckets[owner].push(i);
    }
    let grouped: Vec<_> = buckets.iter().flatten().copied().collect();
    let mut runs = Vec::new();
    for run in 0..4 {
        for bucket in &buckets {
            let n = bucket.len().div_ceil(4);
            let start = (run * n).min(bucket.len());
            let end = ((run + 1) * n).min(bucket.len());
            runs.extend_from_slice(&bucket[start..end]);
        }
    }
    let mut shuffled = grouped.clone();
    let mut state = 0x190_u64;
    for i in (1..shuffled.len()).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        shuffled.swap(i, (state as usize) % (i + 1));
    }
    [grouped, runs, shuffled]
        .into_iter()
        .map(|order| {
            assert_eq!(order.len(), rows.len());
            let mut out = b[..first].to_vec();
            for i in order {
                let (offset, len) = rows[i];
                out.extend_from_slice(&b[offset..offset + len]);
                out.push(b'\n');
            }
            out.extend_from_slice(&b[end..]);
            out
        })
        .collect()
}
