//! COFF COMDAT selection before symbol/section flattening.
use crate::coff::{Coff, Comdat};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
type SectionId = (usize, usize);
pub(crate) struct Selection {
    pub keep: Vec<Vec<bool>>,
    pub replacement: Vec<Vec<Option<SectionId>>>,
    pub discarded: Vec<SectionId>,
}

pub(crate) fn select(files: &[&Coff<'_>]) -> Result<Selection> {
    let metadata = files
        .iter()
        .map(|f| f.comdats())
        .collect::<Result<Vec<_>>>()?;
    let mut roots = Vec::new();
    for entries in &metadata {
        let mut resolved = vec![usize::MAX; entries.len()];
        let mut visiting = vec![false; entries.len()];
        for start in 0..entries.len() {
            if resolved[start] != usize::MAX {
                continue;
            }
            let mut current = start;
            let mut path = Vec::new();
            while resolved[current] == usize::MAX {
                ensure!(!visiting[current], "cyclic associative COMDAT sections");
                if let Some(parent) = entries[current].as_ref().and_then(|m| m.parent) {
                    ensure!(
                        entries[parent].is_some(),
                        "associative COMDAT parent is not COMDAT"
                    );
                    visiting[current] = true;
                    path.push(current);
                    current = parent;
                } else {
                    resolved[current] = current;
                    break;
                }
            }
            let root = resolved[current];
            for item in path {
                resolved[item] = root;
                visiting[item] = false;
            }
        }
        roots.push(resolved);
    }
    let mut groups = BTreeMap::<String, Vec<SectionId>>::new();
    for (fi, entries) in metadata.iter().enumerate() {
        for (si, entry) in entries.iter().enumerate() {
            if let Some(Comdat {
                selection,
                key: Some(key),
                ..
            }) = entry
            {
                if *selection != 5 {
                    groups.entry(key.clone()).or_default().push((fi, si));
                }
            }
        }
    }
    let mut winners = BTreeMap::new();
    for (name, group) in groups {
        let first = group[0];
        let mode = metadata[first.0][first.1].as_ref().unwrap().selection;
        ensure!(
            group
                .iter()
                .all(|&(f, s)| metadata[f][s].as_ref().unwrap().selection == mode),
            "COMDAT {name} has conflicting selection modes"
        );
        let mut winner = first;
        if group.len() > 1 {
            match mode {
                1 => anyhow::bail!("duplicate NODUPLICATES COMDAT: {name}"),
                2 => {}
                3 => ensure!(
                    group.iter().all(|&(f, s)| files[f].sections[s].size
                        == files[first.0].sections[first.1].size),
                    "COMDAT SAME_SIZE mismatch: {name}"
                ),
                4 => {
                    for &candidate in group.iter().skip(1) {
                        ensure!(
                            equivalent(files, first, candidate)?,
                            "COMDAT EXACT_MATCH mismatch: {name}"
                        );
                    }
                }
                6 => {
                    for &candidate in group.iter().skip(1) {
                        if files[candidate.0].sections[candidate.1].size
                            > files[winner.0].sections[winner.1].size
                        {
                            winner = candidate;
                        }
                    }
                }
                7 => {
                    for &candidate in group.iter().skip(1) {
                        if files[candidate.0].timestamp > files[winner.0].timestamp {
                            winner = candidate;
                        }
                    }
                }
                _ => anyhow::bail!("invalid root COMDAT selection"),
            }
        }
        for section in group {
            winners.insert(section, winner);
        }
    }
    let mut keep: Vec<Vec<bool>> = files.iter().map(|f| vec![true; f.sections.len()]).collect();
    let mut replacement: Vec<Vec<Option<SectionId>>> =
        files.iter().map(|f| vec![None; f.sections.len()]).collect();
    let mut discarded = Vec::new();
    for (fi, file) in files.iter().enumerate() {
        for si in 0..file.sections.len() {
            let root = (fi, roots[fi][si]);
            let winner = *winners.get(&root).unwrap_or(&root);
            if root == winner {
                replacement[fi][si] = Some((fi, si));
                continue;
            }
            keep[fi][si] = false;
            discarded.push((fi, si));
            if si == root.1 {
                replacement[fi][si] = Some(winner);
            } else {
                let candidates: Vec<_> = files[winner.0]
                    .sections
                    .iter()
                    .enumerate()
                    .filter(|(n, s)| {
                        roots[winner.0][*n] == winner.1 && s.name == file.sections[si].name
                    })
                    .map(|(n, _)| (winner.0, n))
                    .collect();
                // Only a unique corresponding associative section has a safe mapping.
                if candidates.len() == 1 {
                    replacement[fi][si] = Some(candidates[0]);
                }
            }
        }
    }
    Ok(Selection {
        keep,
        replacement,
        discarded,
    })
}

/// Compare bytes and relocation meaning, not raw per-object symbol indices.
fn equivalent(files: &[&Coff<'_>], left: SectionId, right: SectionId) -> Result<bool> {
    let a = &files[left.0].sections[left.1];
    let b = &files[right.0].sections[right.1];
    if a.size != b.size
        || a.data != b.data
        || a.flags != b.flags
        || a.relocations.len() != b.relocations.len()
    {
        return Ok(false);
    }
    let mut ar: Vec<_> = a.relocations.iter().collect();
    let mut br: Vec<_> = b.relocations.iter().collect();
    ar.sort_by_key(|r| (r.offset, r.typ));
    br.sort_by_key(|r| (r.offset, r.typ));
    fn target(file: &Coff<'_>, section: usize, symbol: usize) -> Result<(String, u32)> {
        let symbol = file
            .symbols
            .get(symbol)
            .and_then(Option::as_ref)
            .context("invalid EXACT_MATCH relocation symbol")?;
        ensure!(
            !symbol.is_weak(),
            "EXACT_MATCH comparison of weak references needs explicit normalization"
        );
        if symbol.storage == 2 {
            return Ok((format!("external:{}", symbol.name), 0));
        }
        if symbol.section == section as i32 + 1 {
            return Ok(("self".into(), symbol.value));
        }
        if symbol.section == -1 {
            return Ok(("absolute".into(), symbol.value));
        }
        anyhow::bail!("EXACT_MATCH comparison requires a cross-section local reference; this case needs explicit normalization")
    }
    for (a, b) in ar.iter().zip(br) {
        if a.offset != b.offset || a.typ != b.typ {
            return Ok(false);
        }
        if a.typ != 0
            && target(files[left.0], left.1, a.symbol)?
                != target(files[right.0], right.1, b.symbol)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}
