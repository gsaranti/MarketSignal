// Shared frontend types mirroring the Rust `GeneratedReport` / `ReportSummary`
// structs returned by the `generate_report_manual` command.

export interface ReportSummary {
  report_id: string;
  report_type: string;
  created_at: string;
  risk_posture: string;
  market_cycle: string;
  thesis_stance: string;
  header_summary_bullets: string[];
  key_risks: string[];
  unresolved_questions: string[];
  forward_outlook_themes: string[];
}

export interface GeneratedReport {
  report_id: string;
  markdown: string;
  markdown_path: string;
  summary: ReportSummary;
}

// Mirrors the Rust `config::ValidationReport` / `WarningCategory` returned by
// the `check_configuration` command and used to gate report generation.
export interface WarningCategory {
  kind: string;
  title: string;
  items: string[];
}

export interface ValidationReport {
  categories: WarningCategory[];
  is_blocked: boolean;
}
