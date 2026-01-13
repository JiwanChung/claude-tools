use serde::Deserialize;

/// Token usage from a session
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_5m_tokens: u64,
    pub cache_creation_1h_tokens: u64,
    pub cache_read_tokens: u64,
}

impl TokenUsage {
    pub fn total_input(&self) -> u64 {
        self.input_tokens + self.cache_creation_5m_tokens + self.cache_creation_1h_tokens + self.cache_read_tokens
    }

    pub fn total(&self) -> u64 {
        self.total_input() + self.output_tokens
    }
}

/// Model pricing in USD per million tokens
#[derive(Debug, Clone)]
pub struct ModelPricing {
    pub input: f64,
    pub output: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
    pub cache_read: f64,
}

impl ModelPricing {
    /// Calculate cost in USD for given token usage
    pub fn calculate_cost(&self, usage: &TokenUsage) -> f64 {
        let mtok = 1_000_000.0;
        (usage.input_tokens as f64 / mtok) * self.input
            + (usage.output_tokens as f64 / mtok) * self.output
            + (usage.cache_creation_5m_tokens as f64 / mtok) * self.cache_write_5m
            + (usage.cache_creation_1h_tokens as f64 / mtok) * self.cache_write_1h
            + (usage.cache_read_tokens as f64 / mtok) * self.cache_read
    }
}

/// Get pricing for a model by ID
/// Pricing from https://platform.claude.com/docs/en/about-claude/pricing
pub fn get_model_pricing(model_id: &str) -> ModelPricing {
    // Default to Sonnet 4 pricing if unknown
    let (input, output) = match model_id {
        // Opus 4.5
        s if s.contains("opus-4-5") || s.contains("opus-4.5") => (5.0, 25.0),
        // Opus 4/4.1
        s if s.contains("opus-4") || s.contains("opus-4.1") => (15.0, 75.0),
        // Sonnet 4.5
        s if s.contains("sonnet-4-5") || s.contains("sonnet-4.5") => (3.0, 15.0),
        // Sonnet 4/3.7
        s if s.contains("sonnet-4") || s.contains("sonnet-3-7") || s.contains("sonnet-3.7") => (3.0, 15.0),
        // Haiku 4.5
        s if s.contains("haiku-4-5") || s.contains("haiku-4.5") => (1.0, 5.0),
        // Haiku 3.5
        s if s.contains("haiku-3-5") || s.contains("haiku-3.5") => (0.8, 4.0),
        // Haiku 3
        s if s.contains("haiku-3") || s.contains("haiku") => (0.25, 1.25),
        // Default to Sonnet 4 pricing
        _ => (3.0, 15.0),
    };

    ModelPricing {
        input,
        output,
        cache_write_5m: input * 1.25,
        cache_write_1h: input * 2.0,
        cache_read: input * 0.1,
    }
}

/// Usage entry from JSONL
#[derive(Deserialize, Debug)]
pub struct JsonlUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation: Option<CacheCreation>,
}

#[derive(Deserialize, Debug)]
pub struct CacheCreation {
    pub ephemeral_5m_input_tokens: Option<u64>,
    pub ephemeral_1h_input_tokens: Option<u64>,
}

#[derive(Deserialize, Debug)]
pub struct JsonlMessage {
    pub model: Option<String>,
    pub usage: Option<JsonlUsage>,
}

#[derive(Deserialize, Debug)]
pub struct JsonlEntry {
    pub message: Option<JsonlMessage>,
}

/// Session usage summary with cost
#[derive(Debug, Clone, Default)]
pub struct SessionCost {
    #[allow(dead_code)]
    pub model: String,
    pub usage: TokenUsage,
    pub cost_usd: f64,
}

/// Parse a JSONL file and calculate total usage and cost
pub fn calculate_session_cost(jsonl_path: &std::path::Path) -> Option<SessionCost> {
    use std::fs::File;
    use std::io::{BufRead, BufReader, Seek, SeekFrom};

    let file = File::open(jsonl_path).ok()?;
    let file_size = file.metadata().ok()?.len();
    let mut reader = BufReader::new(file);

    // Read from end of file for large files
    let start_pos = file_size.saturating_sub(500_000); // Last 500KB
    reader.seek(SeekFrom::Start(start_pos)).ok()?;

    if start_pos > 0 {
        let mut skip = String::new();
        reader.read_line(&mut skip).ok()?;
    }

    let mut usage = TokenUsage::default();
    let mut model = String::new();
    let mut seen_entries = std::collections::HashSet::new();

    for line in reader.lines().flatten() {
        if line.is_empty() {
            continue;
        }

        // Skip duplicates (streaming sends multiple entries with same data)
        let line_hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            line.hash(&mut hasher);
            hasher.finish()
        };
        if seen_entries.contains(&line_hash) {
            continue;
        }
        seen_entries.insert(line_hash);

        if let Ok(entry) = serde_json::from_str::<JsonlEntry>(&line) {
            if let Some(msg) = entry.message {
                if let Some(m) = msg.model {
                    if !m.is_empty() {
                        model = m;
                    }
                }

                if let Some(u) = msg.usage {
                    // Only count entries with output_tokens (final streaming entry)
                    if u.output_tokens.unwrap_or(0) > 0 {
                        usage.input_tokens += u.input_tokens.unwrap_or(0);
                        usage.output_tokens += u.output_tokens.unwrap_or(0);
                        usage.cache_read_tokens += u.cache_read_input_tokens.unwrap_or(0);

                        if let Some(cc) = u.cache_creation {
                            usage.cache_creation_5m_tokens += cc.ephemeral_5m_input_tokens.unwrap_or(0);
                            usage.cache_creation_1h_tokens += cc.ephemeral_1h_input_tokens.unwrap_or(0);
                        } else {
                            // Fallback: count all cache_creation as 5m
                            usage.cache_creation_5m_tokens += u.cache_creation_input_tokens.unwrap_or(0);
                        }
                    }
                }
            }
        }
    }

    if model.is_empty() && usage.total() == 0 {
        return None;
    }

    let pricing = get_model_pricing(&model);
    let cost_usd = pricing.calculate_cost(&usage);

    Some(SessionCost {
        model,
        usage,
        cost_usd,
    })
}

/// Format token count for display (e.g., "1.2k", "3.5M")
pub fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

/// Format USD cost for display
pub fn format_cost(cost: f64) -> String {
    if cost >= 1.0 {
        format!("${:.2}", cost)
    } else if cost >= 0.01 {
        format!("${:.3}", cost)
    } else if cost >= 0.001 {
        format!("${:.4}", cost)
    } else {
        format!("${:.5}", cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pricing_opus_45() {
        let pricing = get_model_pricing("claude-opus-4-5-20251101");
        assert_eq!(pricing.input, 5.0);
        assert_eq!(pricing.output, 25.0);
    }

    #[test]
    fn test_pricing_sonnet() {
        let pricing = get_model_pricing("claude-sonnet-4-20250514");
        assert_eq!(pricing.input, 3.0);
        assert_eq!(pricing.output, 15.0);
    }

    #[test]
    fn test_cost_calculation() {
        let pricing = get_model_pricing("claude-opus-4-5-20251101");
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 100_000,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            cache_read_tokens: 0,
        };
        // 1M input at $5/MTok + 100K output at $25/MTok = $5 + $2.50 = $7.50
        let cost = pricing.calculate_cost(&usage);
        assert!((cost - 7.5).abs() < 0.001);
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(1500), "1.5k");
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }
}
