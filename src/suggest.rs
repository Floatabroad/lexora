pub fn levenshtein(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    let n = b_chars.len();
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];
    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b_chars.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

pub fn nearest<'a, I>(target: &str, candidates: I) -> Option<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let threshold = (target.chars().count() / 3).max(1);
    let mut best: Option<(usize, &'a str)> = None;
    for cand in candidates {
        if cand == target {
            continue;
        }
        let dist = levenshtein(target, cand);
        if dist > threshold {
            continue;
        }
        let better = match best {
            Some((bd, bs)) => dist < bd || (dist == bd && cand < bs),
            None => true,
        };
        if better {
            best = Some((dist, cand));
        }
    }
    best.map(|(_, s)| s.to_string())
}