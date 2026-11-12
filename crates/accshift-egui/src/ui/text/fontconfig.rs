//! Linux faces through fontconfig's own answer: `fc-match` reports the file
//! the desktop configured for a pattern, wherever the distribution put it
//! (Debian, Fedora, openSUSE and Arch all use different directories). The
//! process is asked instead of linking libfontconfig, so the packages keep
//! their dependency list; without `fc-match` a few well-known files remain.
//!
//! The resolution takes its runner and its file check as parameters: the
//! tests feed it the answers of other distributions.

use std::path::{Path, PathBuf};

use super::faces::{Entry, Source};

/// Catalog name of the face fontconfig picks for Korean, which the Hangul
/// fallback asks for after the platforms' own Korean faces.
pub const SYSTEM_HANGUL: &str = "accshift system hangul";

/// Catalog names of the faces fontconfig picks for arrows and maths, and for
/// the other symbol blocks. The desktop's sans may hold none of them (Noto
/// Sans, Ubuntu's default, has no arrow) and egui's own fonts have no arrow
/// either, so the symbol fallbacks ask for a face by what it must draw.
pub const SYSTEM_MATH: &str = "accshift system math";
pub const SYSTEM_SYMBOL: &str = "accshift system symbol";

/// The files tried for the symbol faces without fontconfig: DejaVu Sans
/// draws the arrows and the dingbats, wherever the distribution put it.
const DEJAVU_SANS: &[(&str, u32, u16)] = &[
    ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 0, 400),
    (
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        0,
        700,
    ),
    ("/usr/share/fonts/TTF/DejaVuSans.ttf", 0, 400),
    ("/usr/share/fonts/TTF/DejaVuSans-Bold.ttf", 0, 700),
    ("/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf", 0, 400),
    (
        "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf",
        0,
        700,
    ),
];

/// One catalog family resolved through fontconfig.
pub struct Query {
    /// The catalog names it answers (normalized CSS family names).
    pub names: &'static [&'static str],
    /// The fontconfig pattern, without a weight.
    pub pattern: &'static str,
    /// The CSS weights asked for.
    pub weights: &'static [u16],
    /// Files tried when fontconfig gives nothing: path, face index, weight.
    pub fixed: &'static [(&'static str, u32, u16)],
}

/// The families the stacks need off Windows and macOS. The CJK, Hangul and
/// symbol ones are only resolved once such text shows up. Noto Sans CJK
/// collections hold JP, KR, SC, TC and HK, in that order.
pub const QUERIES: &[Query] = &[
    Query {
        names: &["system-ui", "sans-serif", "ui-sans-serif"],
        pattern: "sans-serif",
        weights: &[400, 600, 700],
        fixed: DEJAVU_SANS,
    },
    Query {
        names: &["monospace", "ui-monospace"],
        pattern: "monospace",
        weights: &[400, 700],
        fixed: &[
            (
                "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
                0,
                400,
            ),
            ("/usr/share/fonts/TTF/DejaVuSansMono.ttf", 0, 400),
            (
                "/usr/share/fonts/dejavu-sans-mono-fonts/DejaVuSansMono.ttf",
                0,
                400,
            ),
        ],
    },
    // The stack's system CJK slot: whatever the desktop draws Chinese with.
    Query {
        names: &["noto sans cjk sc"],
        pattern: "sans-serif:lang=zh-cn",
        weights: &[400, 700],
        fixed: &[
            (
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                2,
                400,
            ),
            (
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
                2,
                700,
            ),
            ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 2, 400),
            ("/usr/share/fonts/noto-cjk/NotoSansCJK-Bold.ttc", 2, 700),
            (
                "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
                2,
                400,
            ),
            (
                "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Bold.ttc",
                2,
                700,
            ),
        ],
    },
    Query {
        names: &[SYSTEM_HANGUL],
        pattern: "sans-serif:lang=ko",
        weights: &[400, 700],
        fixed: &[
            (
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                1,
                400,
            ),
            (
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
                1,
                700,
            ),
            ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 1, 400),
            ("/usr/share/fonts/noto-cjk/NotoSansCJK-Bold.ttc", 1, 700),
            (
                "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
                1,
                400,
            ),
            (
                "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Bold.ttc",
                1,
                700,
            ),
        ],
    },
    // A charset outranks the family in fontconfig's matching: the answer is
    // the sans that draws these characters. Arrows, the reset arrow, minus
    // and a geometric shape; then a star, a check, a cross and a heart.
    Query {
        names: &[SYSTEM_MATH],
        pattern: "sans-serif:charset=2190 2192 21ba 2212 25b6",
        weights: &[400, 700],
        fixed: DEJAVU_SANS,
    },
    Query {
        names: &[SYSTEM_SYMBOL],
        pattern: "sans-serif:charset=2605 2713 2715 2665",
        weights: &[400, 700],
        fixed: DEJAVU_SANS,
    },
];

/// The fontconfig pattern of `pattern` at a CSS weight (`FC_WEIGHT_*`).
pub fn pattern_at(pattern: &str, weight: u16) -> String {
    let fc = match weight {
        0..=150 => 0,
        151..=250 => 40,
        251..=325 => 50,
        326..=375 => 55,
        376..=450 => 80,
        451..=550 => 100,
        551..=650 => 180,
        651..=750 => 200,
        751..=850 => 205,
        _ => 210,
    };
    format!("{pattern}:weight={fc}")
}

/// The `fc-match -f '%{file}\n%{index}'` answer: the file and its face
/// index. The upper 16 bits of fontconfig's index name a variable font's
/// instance, which the catalog finds from the file itself.
pub fn parse_match(out: &str) -> Option<(PathBuf, u32)> {
    let mut lines = out.lines();
    let file = lines.next()?.trim();
    if file.is_empty() {
        return None;
    }
    let index = lines
        .next()
        .and_then(|i| i.trim().parse::<u32>().ok())
        .unwrap_or(0);
    Some((PathBuf::from(file), index & 0xFFFF))
}

/// The faces of `query`: fontconfig's answer at each weight (`run` gets the
/// patterns and returns one output per pattern), else the fixed files that
/// exist. A file answered for several weights is kept once, at the first:
/// a variable font then gets its weights from its named instances, and a
/// family without a bold face does not pass its regular for one.
pub fn resolve(
    query: &Query,
    run: &mut dyn FnMut(&[String]) -> Vec<Option<String>>,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<Entry> {
    let patterns: Vec<String> = query
        .weights
        .iter()
        .map(|&w| pattern_at(query.pattern, w))
        .collect();
    let answers = run(&patterns);
    let mut out: Vec<Entry> = Vec::new();
    for (&weight, answer) in query.weights.iter().zip(answers) {
        let Some((path, index)) = answer.as_deref().and_then(parse_match) else {
            continue;
        };
        let duplicate = out
            .iter()
            .any(|e| e.index == index && e.source == Source::File(path.clone()));
        if !duplicate && exists(&path) {
            out.push(Entry {
                source: Source::File(path),
                index,
                weight,
            });
        }
    }
    if out.is_empty() {
        for &(path, index, weight) in query.fixed {
            let path = PathBuf::from(path);
            if exists(&path) {
                out.push(Entry {
                    source: Source::File(path),
                    index,
                    weight,
                });
            }
        }
    }
    out
}

/// Runs `fc-match` once per pattern, all at the same time; `None` where it
/// is missing or fails.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn fc_match(patterns: &[String]) -> Vec<Option<String>> {
    use std::process::{Command, Stdio};
    let children: Vec<_> = patterns
        .iter()
        .map(|p| {
            Command::new("fc-match")
                .args(["-f", "%{file}\n%{index}", p])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok()
        })
        .collect();
    children
        .into_iter()
        .map(|child| {
            let out = child?.wait_with_output().ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(names: &[&str]) -> &'static Query {
        QUERIES
            .iter()
            .find(|q| q.names == names)
            .expect("known query")
    }

    /// Answers like a desktop whose fontconfig knows `files`, by pattern.
    fn fake<'a>(
        answers: &'a [(&'a str, &'a str)],
    ) -> impl FnMut(&[String]) -> Vec<Option<String>> + 'a {
        move |patterns| {
            patterns
                .iter()
                .map(|p| {
                    answers
                        .iter()
                        .find(|(pattern, _)| pattern == p)
                        .map(|(_, out)| out.to_string())
                })
                .collect()
        }
    }

    #[test]
    fn fedora_paths_come_from_fontconfig() {
        let regular = "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf";
        let bold = "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf";
        let cjk = "/usr/share/fonts/google-noto-sans-cjk-vf-fonts/NotoSansCJK-VF.ttc";
        let answers = [
            ("sans-serif:weight=80", format!("{regular}\n0")),
            ("sans-serif:weight=180", format!("{bold}\n0")),
            ("sans-serif:weight=200", format!("{bold}\n0")),
            // Instance 3 of face 2: the upper bits are dropped.
            (
                "sans-serif:lang=zh-cn:weight=80",
                format!("{cjk}\n{}", (3 << 16) | 2),
            ),
            (
                "sans-serif:lang=zh-cn:weight=200",
                format!("{cjk}\n{}", (7 << 16) | 2),
            ),
        ];
        let answers: Vec<(&str, &str)> = answers.iter().map(|(p, o)| (*p, o.as_str())).collect();
        let fedora = [regular, bold, cjk];
        let exists = |p: &Path| fedora.iter().any(|f| Path::new(f) == p);

        let sans = resolve(
            query(&["system-ui", "sans-serif", "ui-sans-serif"]),
            &mut fake(&answers),
            &exists,
        );
        assert_eq!(
            sans,
            vec![
                Entry {
                    source: Source::File(regular.into()),
                    index: 0,
                    weight: 400
                },
                Entry {
                    source: Source::File(bold.into()),
                    index: 0,
                    weight: 600
                },
            ]
        );
        let han = resolve(query(&["noto sans cjk sc"]), &mut fake(&answers), &exists);
        assert_eq!(
            han,
            vec![Entry {
                source: Source::File(cjk.into()),
                index: 2,
                weight: 400
            }],
            "one variable file, kept once"
        );
    }

    #[test]
    fn fixed_files_without_fontconfig() {
        let debian = "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc";
        let exists = |p: &Path| p == Path::new(debian);
        let mut missing = |patterns: &[String]| vec![None; patterns.len()];
        let ko = resolve(query(&[SYSTEM_HANGUL]), &mut missing, &exists);
        assert_eq!(
            ko,
            vec![Entry {
                source: Source::File(debian.into()),
                index: 1,
                weight: 400
            }]
        );
        assert!(resolve(query(&["monospace", "ui-monospace"]), &mut missing, &exists).is_empty());
    }

    /// Ubuntu: the sans is Noto Sans, which has no arrow, and fontconfig
    /// answers DejaVu Sans once the pattern names the characters.
    #[test]
    fn the_symbol_faces_come_from_a_charset_pattern() {
        let noto = "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf";
        let regular = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf";
        let bold = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf";
        let answers = [
            (
                "sans-serif:weight=80",
                format!(
                    "{noto}
0"
                ),
            ),
            (
                "sans-serif:charset=2190 2192 21ba 2212 25b6:weight=80",
                format!(
                    "{regular}
0"
                ),
            ),
            (
                "sans-serif:charset=2190 2192 21ba 2212 25b6:weight=200",
                format!(
                    "{bold}
0"
                ),
            ),
            (
                "sans-serif:charset=2605 2713 2715 2665:weight=80",
                format!(
                    "{regular}
0"
                ),
            ),
        ];
        let answers: Vec<(&str, &str)> = answers.iter().map(|(p, o)| (*p, o.as_str())).collect();
        let exists = |p: &Path| [noto, regular, bold].iter().any(|f| Path::new(f) == p);
        let file = |path: &str, weight| Entry {
            source: Source::File(path.into()),
            index: 0,
            weight,
        };

        let math = resolve(query(&[SYSTEM_MATH]), &mut fake(&answers), &exists);
        assert_eq!(math, vec![file(regular, 400), file(bold, 700)]);
        let symbol = resolve(query(&[SYSTEM_SYMBOL]), &mut fake(&answers), &exists);
        assert_eq!(symbol, vec![file(regular, 400)]);

        // Without fontconfig, the DejaVu files that exist.
        let mut missing = |patterns: &[String]| vec![None; patterns.len()];
        let fixed = resolve(query(&[SYSTEM_MATH]), &mut missing, &exists);
        assert_eq!(fixed, vec![file(regular, 400), file(bold, 700)]);
    }

    #[test]
    fn answers_for_missing_files_are_ignored() {
        let mut gone = fake(&[("monospace:weight=80", "/nowhere/Mono.ttf\n0")]);
        assert!(resolve(query(&["monospace", "ui-monospace"]), &mut gone, &|_| false).is_empty());
        assert_eq!(parse_match(""), None);
        assert_eq!(
            parse_match("/a/b.ttf"),
            Some((PathBuf::from("/a/b.ttf"), 0))
        );
    }
}
