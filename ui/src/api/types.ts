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
  /** Whether setup (install, Start menu) is built for this platform. */
  setup?: boolean;
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
  redact(source: Source, action?: string, unverified?: boolean): Promise<Redaction>;
  setup(): Promise<SetupStatus>;
  install(request: SetupRequest): Promise<SetupStatus>;
  /** Removes setup's work; veilq ends, and its folder goes after it. */
  uninstall(models: boolean): Promise<void>;
  models(): Promise<Models>;
  /** Starts a download; `models()` shows how far it is. */
  fetchModel(name: string): Promise<void>;
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
