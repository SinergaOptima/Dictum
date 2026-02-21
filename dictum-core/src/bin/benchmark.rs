#[cfg(not(feature = "onnx"))]
fn main() {
    eprintln!("dictum benchmark requires the 'onnx' feature");
    std::process::exit(1);
}

#[cfg(feature = "onnx")]
fn main() {
    if let Err(e) = run() {
        eprintln!("benchmark failed: {e}");
        std::process::exit(1);
    }
}

#[cfg(feature = "onnx")]
fn run() -> Result<(), String> {
    use dictum_core::{buffering::chunk::AudioChunk, OnnxModel, OnnxModelConfig, SpeechModel};
    use serde::Serialize;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    #[derive(Debug)]
    struct Args {
        fixtures_dir: PathBuf,
        iterations: usize,
        output: Option<PathBuf>,
    }

    #[derive(Debug, Clone, Serialize)]
    struct CaseResult {
        file: String,
        category: String,
        iteration: usize,
        latency_ms: f64,
        text_len: usize,
        confidence: Option<f32>,
        is_empty: bool,
        used_placeholder: bool,
        similarity_to_expected: Option<f32>,
    }

    #[derive(Debug, Clone, Serialize)]
    struct CategorySummary {
        category: String,
        runs: usize,
        p50_latency_ms: f64,
        p95_latency_ms: f64,
        avg_latency_ms: f64,
        miss_rate: f64,
        placeholder_rate: f64,
        avg_confidence: Option<f32>,
        avg_similarity_to_expected: Option<f32>,
    }

    #[derive(Debug, Clone, Serialize)]
    struct Summary {
        fixtures_dir: String,
        iterations: usize,
        total_runs: usize,
        total_files: usize,
        p50_latency_ms: f64,
        p95_latency_ms: f64,
        avg_latency_ms: f64,
        miss_rate: f64,
        placeholder_rate: f64,
        avg_confidence: Option<f32>,
        avg_similarity_to_expected: Option<f32>,
        categories: Vec<CategorySummary>,
        cases: Vec<CaseResult>,
    }

    fn parse_args() -> Result<Args, String> {
        let mut fixtures_dir: Option<PathBuf> = None;
        let mut iterations: usize = 1;
        let mut output: Option<PathBuf> = None;

        let mut it = std::env::args().skip(1).peekable();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--fixtures" => {
                    let Some(v) = it.next() else {
                        return Err("missing value for --fixtures".into());
                    };
                    fixtures_dir = Some(PathBuf::from(v));
                }
                "--iterations" => {
                    let Some(v) = it.next() else {
                        return Err("missing value for --iterations".into());
                    };
                    iterations = v
                        .parse::<usize>()
                        .map_err(|_| "invalid value for --iterations".to_string())?
                        .clamp(1, 10);
                }
                "--output" => {
                    let Some(v) = it.next() else {
                        return Err("missing value for --output".into());
                    };
                    output = Some(PathBuf::from(v));
                }
                "--help" | "-h" => {
                    println!(
                        "Usage: cargo run -p dictum-core --features onnx --bin benchmark -- \\
  --fixtures <dir> [--iterations <n>] [--output <file.json>]"
                    );
                    std::process::exit(0);
                }
                other => {
                    return Err(format!("unknown argument: {other}"));
                }
            }
        }

        let fixtures_dir = fixtures_dir.unwrap_or_else(|| PathBuf::from("benchmarks/fixtures"));
        Ok(Args {
            fixtures_dir,
            iterations,
            output,
        })
    }

    fn collect_wavs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                collect_wavs(&path, out)?;
                continue;
            }
            let is_wav = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("wav"))
                .unwrap_or(false);
            if is_wav {
                out.push(path);
            }
        }
        Ok(())
    }

    fn read_wav_mono_f32(path: &Path) -> Result<(Vec<f32>, u32), String> {
        let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
        let spec = reader.spec();
        let channels = usize::from(spec.channels.max(1));

        let interleaved: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader
                .samples::<f32>()
                .map(|s| s.map_err(|e| e.to_string()))
                .collect::<Result<Vec<_>, _>>()?,
            hound::SampleFormat::Int => {
                if spec.bits_per_sample <= 16 {
                    reader
                        .samples::<i16>()
                        .map(|s| {
                            s.map(|v| (v as f32) / (i16::MAX as f32))
                                .map_err(|e| e.to_string())
                        })
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    let max = ((1_i64 << (spec.bits_per_sample - 1)) - 1) as f32;
                    reader
                        .samples::<i32>()
                        .map(|s| s.map(|v| (v as f32) / max).map_err(|e| e.to_string()))
                        .collect::<Result<Vec<_>, _>>()?
                }
            }
        };

        if channels == 1 {
            return Ok((interleaved, spec.sample_rate));
        }

        let mut mono = Vec::with_capacity(interleaved.len() / channels);
        for frame in interleaved.chunks(channels) {
            let sum = frame.iter().copied().sum::<f32>();
            mono.push(sum / channels as f32);
        }
        Ok((mono, spec.sample_rate))
    }

    fn category_for(path: &Path) -> String {
        let joined = path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join("/");
        if joined.contains("quiet") {
            "quiet_speech".into()
        } else if joined.contains("whisper") {
            "whisper_speech".into()
        } else if joined.contains("noisy") || joined.contains("noise") {
            "noisy_room".into()
        } else if joined.contains("long") {
            "long_form".into()
        } else {
            "other".into()
        }
    }

    fn expected_text_for(path: &Path) -> Option<String> {
        let expected = path.with_extension("txt");
        std::fs::read_to_string(expected)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    }

    fn normalize_words(text: &str) -> Vec<String> {
        text.split_whitespace()
            .map(|w| {
                w.chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '\'')
                    .collect::<String>()
                    .to_ascii_lowercase()
            })
            .filter(|w| !w.is_empty())
            .collect()
    }

    fn overlap_similarity(expected: &str, actual: &str) -> Option<f32> {
        let ref_words = normalize_words(expected);
        let hyp_words = normalize_words(actual);
        if ref_words.is_empty() || hyp_words.is_empty() {
            return None;
        }
        let mut matched = 0usize;
        let span = ref_words.len().max(hyp_words.len());
        let cmp_len = ref_words.len().min(hyp_words.len());
        for i in 0..cmp_len {
            if ref_words[i] == hyp_words[i] {
                matched += 1;
            }
        }
        Some((matched as f32 / span as f32).clamp(0.0, 1.0))
    }

    fn percentile(values: &[f64], p: f64) -> f64 {
        if values.is_empty() {
            return 0.0;
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.total_cmp(b));
        if sorted.len() == 1 {
            return sorted[0];
        }
        let idx = ((sorted.len() - 1) as f64 * p.clamp(0.0, 1.0)).round() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    fn summarize(category: String, rows: &[CaseResult]) -> CategorySummary {
        let latencies = rows.iter().map(|r| r.latency_ms).collect::<Vec<_>>();
        let avg_latency_ms = if latencies.is_empty() {
            0.0
        } else {
            latencies.iter().sum::<f64>() / latencies.len() as f64
        };
        let miss_count = rows.iter().filter(|r| r.is_empty).count();
        let placeholder_count = rows.iter().filter(|r| r.used_placeholder).count();
        let confidences = rows.iter().filter_map(|r| r.confidence).collect::<Vec<_>>();
        let similarities = rows
            .iter()
            .filter_map(|r| r.similarity_to_expected)
            .collect::<Vec<_>>();

        CategorySummary {
            category,
            runs: rows.len(),
            p50_latency_ms: percentile(&latencies, 0.50),
            p95_latency_ms: percentile(&latencies, 0.95),
            avg_latency_ms,
            miss_rate: if rows.is_empty() {
                0.0
            } else {
                miss_count as f64 / rows.len() as f64
            },
            placeholder_rate: if rows.is_empty() {
                0.0
            } else {
                placeholder_count as f64 / rows.len() as f64
            },
            avg_confidence: if confidences.is_empty() {
                None
            } else {
                Some(confidences.iter().sum::<f32>() / confidences.len() as f32)
            },
            avg_similarity_to_expected: if similarities.is_empty() {
                None
            } else {
                Some(similarities.iter().sum::<f32>() / similarities.len() as f32)
            },
        }
    }

    let args = parse_args()?;
    if !args.fixtures_dir.exists() {
        return Err(format!(
            "fixtures directory not found: {}",
            args.fixtures_dir.display()
        ));
    }

    let mut wav_files = Vec::new();
    collect_wavs(&args.fixtures_dir, &mut wav_files)?;
    wav_files.sort();
    if wav_files.is_empty() {
        return Err(format!(
            "no .wav fixtures found in {}",
            args.fixtures_dir.display()
        ));
    }

    println!(
        "Running Dictum benchmark on {} fixtures (iterations={})",
        wav_files.len(),
        args.iterations
    );

    let mut model = OnnxModel::new(OnnxModelConfig::default());
    model.warm_up().map_err(|e| e.to_string())?;

    let mut cases = Vec::new();
    for wav in &wav_files {
        let (samples, sample_rate) = read_wav_mono_f32(wav)?;
        let chunk = AudioChunk::new(samples, sample_rate);
        let expected = expected_text_for(wav);
        let category = category_for(wav);
        let file = wav
            .strip_prefix(&args.fixtures_dir)
            .unwrap_or(wav)
            .display()
            .to_string();

        for iteration in 1..=args.iterations {
            let started = Instant::now();
            let segments = model
                .transcribe(&chunk, false)
                .map_err(|e| format!("{}: {e}", wav.display()))?;
            let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
            let final_segments = segments
                .iter()
                .filter(|seg| seg.kind == dictum_core::ipc::events::SegmentKind::Final)
                .collect::<Vec<_>>();
            let text = final_segments
                .iter()
                .map(|seg| seg.text.trim())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let confidence = final_segments
                .iter()
                .filter_map(|seg| seg.confidence)
                .next();
            let similarity_to_expected = expected
                .as_ref()
                .and_then(|exp| overlap_similarity(exp, &text));
            let used_placeholder = text.trim().eq_ignore_ascii_case("[speech captured]");
            cases.push(CaseResult {
                file: file.clone(),
                category: category.clone(),
                iteration,
                latency_ms,
                text_len: text.len(),
                confidence,
                is_empty: text.trim().is_empty(),
                used_placeholder,
                similarity_to_expected,
            });
            println!(
                "{file} [{iteration}/{iters}] {latency:.1} ms",
                iters = args.iterations,
                latency = latency_ms
            );
        }
    }

    let mut grouped: BTreeMap<String, Vec<CaseResult>> = BTreeMap::new();
    for row in &cases {
        grouped
            .entry(row.category.clone())
            .or_default()
            .push(row.clone());
    }
    let mut categories = Vec::new();
    for (name, rows) in grouped {
        categories.push(summarize(name, &rows));
    }

    let all_latencies = cases.iter().map(|r| r.latency_ms).collect::<Vec<_>>();
    let all_conf = cases
        .iter()
        .filter_map(|r| r.confidence)
        .collect::<Vec<_>>();
    let all_sim = cases
        .iter()
        .filter_map(|r| r.similarity_to_expected)
        .collect::<Vec<_>>();
    let summary = Summary {
        fixtures_dir: args.fixtures_dir.display().to_string(),
        iterations: args.iterations,
        total_runs: cases.len(),
        total_files: wav_files.len(),
        p50_latency_ms: percentile(&all_latencies, 0.50),
        p95_latency_ms: percentile(&all_latencies, 0.95),
        avg_latency_ms: if all_latencies.is_empty() {
            0.0
        } else {
            all_latencies.iter().sum::<f64>() / all_latencies.len() as f64
        },
        miss_rate: if cases.is_empty() {
            0.0
        } else {
            cases.iter().filter(|r| r.is_empty).count() as f64 / cases.len() as f64
        },
        placeholder_rate: if cases.is_empty() {
            0.0
        } else {
            cases.iter().filter(|r| r.used_placeholder).count() as f64 / cases.len() as f64
        },
        avg_confidence: if all_conf.is_empty() {
            None
        } else {
            Some(all_conf.iter().sum::<f32>() / all_conf.len() as f32)
        },
        avg_similarity_to_expected: if all_sim.is_empty() {
            None
        } else {
            Some(all_sim.iter().sum::<f32>() / all_sim.len() as f32)
        },
        categories,
        cases,
    };

    println!(
        "Done. runs={} p50={:.1}ms p95={:.1}ms miss_rate={:.1}%",
        summary.total_runs,
        summary.p50_latency_ms,
        summary.p95_latency_ms,
        summary.miss_rate * 100.0
    );

    let json = serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())?;
    if let Some(out) = args.output {
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&out, json).map_err(|e| e.to_string())?;
        println!("Wrote benchmark report: {}", out.display());
    } else {
        println!("{json}");
    }

    Ok(())
}
