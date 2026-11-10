// The internal API of `veilq ui` (crates/veilq/src/ui.rs). Findings and the
// report follow schema/report.schema.json.

export type Location =
  | { kind: 'span'; part?: string; start: number; end: number; line: number; column: number }
  | { kind: 'cell'; sheet?: string; row: number; column: number; start: number; end: number }
  | { kind: 'region'; part?: string; frame?: number; x: number; y: number; width: number; height: number };

export interface Finding {
  entity: string;
  location: Location;
  confidence: number;
  detector: string;
  pseudonym?: string;
}

export interface Report {
  version: number;
  findings: Finding[];
  review_required: boolean;
  review_reasons?: string[];
}

export interface Info {
  version: string;
  root: string;
  policy: string;
  default_action: string;
  /** What a run can choose instead of the policy's actions. */
  actions: string[];
  max_input: number;
}

export interface Entry {
  name: string;
  /** Relative to the root, with `/`. */
  path: string;
  dir: boolean;
  size: number;
}

export interface Listing {
  dir: string;
  entries: Entry[];
  truncated: boolean;
}

export type Source =
  | { kind: 'text'; text: string }
  | { kind: 'upload'; name: string; data: string }
  | { kind: 'file'; path: string };

export interface Redaction {
  name?: string;
  input: string;
  output: string;
  /** Per finding, its byte range in `input`. */
  input_spans: [number, number][];
  /** Per finding, the byte range of its replacement in `output`. */
  output_spans: [number, number][];
  report: Report;
  /** An Office file: the redacted file, base64; `input` and `output` are empty. */
  output_base64?: string;
}

export interface Api {
  readonly mock: boolean;
  info(): Promise<Info>;
  files(dir: string): Promise<Listing>;
  /** `action`: overrides the policy's actions for this run. */
  redact(source: Source, action?: string): Promise<Redaction>;
}

export class ApiError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}
