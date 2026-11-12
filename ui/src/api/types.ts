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
  /** Started by a double click: the page says it is open, and can quit veilq. */
  desktop?: boolean;
  /** Whether setup (install, Start menu) is built for this platform and offered. */
  setup?: boolean;
  /** Installed from the Microsoft Store, which updates and removes veilq (ADR-0039). */
  store?: boolean;
  /** Shown in veilq's own window: closing it ends veilq. */
  window?: boolean;
  /** The models are still loading; a run waits for them (ADR-0032). */
  loading?: boolean;
  /** Why the models did not load. */
  load_error?: string;
}

/** What setup shows (ADR-0027). */
export interface SetupStatus {
  available: boolean;
  exe: string;
  install_dir: string;
  installed: boolean;
  runs_installed: boolean;
  start_menu: boolean;
  desktop: boolean;
  on_path: boolean;
}

/** A policy as `veilq.toml` holds it (schema/policy.schema.json); the form edits part of it. */
export interface PolicyPattern {
  name: string;
  entity: string;
  regex: string;
  confidence?: number;
  context?: { words: string[]; boost?: number; window?: number };
  validate?: string;
}

export interface Policy {
  version: number;
  /** What veilq is used for (ADR-0040); `general` is always one. */
  purposes?: string[];
  defaults: { action: string; min_confidence: number };
  locales: string[];
  entities: Record<string, { action?: string; min_confidence?: number }>;
  patterns: PolicyPattern[];
  allow: { values: string[]; patterns: string[] };
  [other: string]: unknown;
}

export interface PolicyView {
  /** The policy in use and where it came from. */
  source: string;
  user_file: string | null;
  /** The user's file is the policy in use (no project's veilq.toml comes first). */
  user_in_use: boolean;
  policy: Policy;
  entities: string[];
  locales: string[];
  actions: string[];
  validators: string[];
  /** The purposes a policy can name, each with what it adds to `general`. */
  purposes: PurposeInfo[];
}

/** A purpose (ADR-0040): what it is for and what it changes against `general`. */
export interface PurposeInfo {
  name: string;
  about: string;
  entities: Record<string, { action?: string; min_confidence?: number }>;
  /** Its patterns: name and type. */
  patterns: [string, string][];
  /** How many values and patterns it takes for no personal data. */
  allow: number;
  costs: Record<string, { miss: number; false_positive: number }>;
}

export interface PolicyTried {
  toml: string;
  output?: string;
  findings?: string[];
}

export interface Folders {
  path: string;
  parent: string | null;
  folders: { name: string; path: string }[];
}

export interface BatchFile {
  path: string;
  findings?: number;
  types?: string[];
  review?: boolean;
  error?: string;
}

export interface Batch {
  source: string;
  target: string;
  total: number;
  done: number;
  current: string | null;
  running: boolean;
  cancelled: boolean;
  files: BatchFile[];
}

/** The update check (ADR-0027): only when asked. */
export interface UpdateCheck {
  current: string;
  latest: string;
  newer: boolean;
  url: string;
  /** This platform's archive in that release, if it has one. */
  archive?: string;
}

export interface SetupRequest {
  start_menu: boolean;
  desktop: boolean;
  path: boolean;
}

export type ModelJob =
  | { state: 'running'; file: string; index: number; of: number; bytes: number }
  | { state: 'done' }
  | { state: 'failed'; error: string };

export interface ModelComponent {
  name: string;
  about: string;
  installed: boolean;
  available: boolean;
  /** The running engine uses it. */
  active: boolean;
  files: { name: string; url: string; installed: boolean }[];
  job?: ModelJob;
}

export interface Models {
  dir: string;
  components: ModelComponent[];
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
  /** An Office file, image or PDF: the redacted file, base64; `input` and `output` are empty. */
  output_base64?: string;
  /** An image or PDF: the input, base64. */
  input_base64?: string;
  /** An image or PDF: its media type. */
  media_type?: string;
  /** A PDF: its first pages before and after, as PNG, in its findings' pixels. */
  input_pages?: string[];
  output_pages?: string[];
  page_count?: number;
  /** What veilq read and why it decided, when asked for (ADR-0034); the values in clear. */
  explain?: Reading[];
}

/** One text veilq read (a line, a cell, a picture's OCR line) and every candidate in it. */
export interface Reading {
  text: string;
  /** What stood before the text and was read as its context. */
  context?: string;
  candidates: Explained[];
}

export type Verdict =
  | 'kept'
  | 'below_threshold'
  | 'allowed'
  | 'overlap'
  | 'role_words'
  | 'account'
  | 'brand_word'
  | 'placeholder';

/** A candidate in a reading, with the policy's verdict on it. */
export interface Explained {
  start: number;
  end: number;
  value: string;
  entity: string;
  detector: string;
  confidence: number;
  threshold: number;
  verdict: Verdict;
  /** A birth date beside a name lifted its confidence. */
  raised?: boolean;
}

export interface Picture {
  /** An image as it is; a PDF's first pages as PNG. */
  pages: string[];
  media_type: string;
  /** A PDF's number of pages. */
  page_count?: number;
}

export interface Api {
  readonly mock: boolean;
  info(): Promise<Info>;
  files(dir: string): Promise<Listing>;
  /** An image under the root as it is, shown while it is redacted. */
  picture(path: string): Promise<Picture>;
  /** `action`: overrides the policy's actions for this run. */
  /**
   * `unverified`: a picture whose verify rounds do not settle (409) is
   * written anyway, with the last round's findings filled, for a review.
   */
  redact(source: Source, action?: string, unverified?: boolean, explain?: boolean): Promise<Redaction>;
  setup(): Promise<SetupStatus>;
  install(request: SetupRequest): Promise<SetupStatus>;
  /** Removes setup's work; veilq ends, and its folder goes after it. */
  uninstall(models: boolean): Promise<void>;
  models(): Promise<Models>;
  /** Starts a download; `models()` shows how far it is. */
  fetchModel(name: string): Promise<void>;
  policy(): Promise<PolicyView>;
  /** Checks the policy and tries it on a sample, with the patterns alone. */
  tryPolicy(policy: Policy, sample?: string): Promise<PolicyTried>;
  /** Writes the user's policy; `in_use`: runs use it now. */
  savePolicy(policy: Policy): Promise<{ path: string; in_use: boolean }>;
  folders(path: string): Promise<Folders>;
  setRoot(path: string): Promise<{ root: string }>;
  batch(): Promise<Batch | null>;
  startBatch(request: { source: string; target: string; action?: string; unverified?: boolean }): Promise<Batch>;
  cancelBatch(): Promise<void>;
  /** Asks GitHub for the newest release; nothing else is sent. */
  checkUpdate(): Promise<UpdateCheck>;
  /** Installs it where veilq runs and starts it; this veilq ends. */
  installUpdate(): Promise<UpdateCheck>;
  /** Ends veilq. */
  quit(): Promise<void>;
  /** A desktop window says it is open. */
  ping(): Promise<void>;
}

export class ApiError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}
