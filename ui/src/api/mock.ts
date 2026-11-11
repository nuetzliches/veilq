// The in-browser fake of `veilq ui`'s API (`npm run dev`, build:mock): a few
// files with synthetic data and a toy detector, so the UI can be built before
// the detectors of M1 exist. Every value here is made up.

import { ApiError, type Api, type Entry, type Finding, type Reading, type Redaction, type Source } from './types';

const FILES: Record<string, string> = {
  'exports/app.log': [
    '2026-11-02T08:14:03Z INFO  login ok user=lena.vogt@example.org ip=203.0.113.24',
    '2026-11-02T08:14:09Z INFO  payment queued iban=DE44 5001 0517 5407 3249 31 amount=120.00',
    '2026-11-02T08:15:41Z WARN  callback failed phone=+49 30 5550182 user=lena.vogt@example.org',
    '2026-11-02T08:16:02Z INFO  record 4711 opened by Jonas Brandt',
    '',
  ].join('\n'),
  'exports/customers.csv': [
    'id,name,email,phone',
    '1,Mira Albers,mira.albers@example.com,0421 5550173',
    '2,Tobias Kern,t.kern@example.net,+49 89 5550198',
    '3,Ida Sommer,ida.sommer@example.org,030 5550142',
    '',
  ].join('\n'),
  'letters/reminder.txt': [
    'Sehr geehrte Frau Albers,',
    '',
    'wir bitten um Ausgleich der offenen Rechnung auf das Konto',
    'DE89 3704 0044 0532 0130 00. Rückfragen gern an buchhaltung@example.com',
    'oder telefonisch unter 0421 5550100.',
    '',
    'Mit freundlichen Grüßen',
    'Jonas Brandt',
    '',
  ].join('\n'),
  'README.txt': 'Synthetic sample files of the veilq UI mock. No real person is named here.\n',
};

/** The names the toy "NER" knows: the ones in the samples. */
const NAMES = ['Lena Vogt', 'Jonas Brandt', 'Mira Albers', 'Tobias Kern', 'Ida Sommer', 'Albers'];

const DETECTORS: { entity: string; regex: RegExp; detector: string; confidence: number }[] = [
  { entity: 'EMAIL_ADDRESS', regex: /[\w.+-]+@[\w-]+(?:\.[\w-]+)+/g, detector: 'pattern', confidence: 1 },
  { entity: 'IBAN_CODE', regex: /\bDE\d{2}(?: ?\d{4}){4} ?\d{2}\b/g, detector: 'pattern', confidence: 1 },
  { entity: 'PHONE_NUMBER', regex: /(?:\+49 ?|\b0)\d{2,4}[ /]?\d{5,8}\b/g, detector: 'pattern', confidence: 0.85 },
  { entity: 'IP_ADDRESS', regex: /\b\d{1,3}(?:\.\d{1,3}){3}\b/g, detector: 'pattern', confidence: 0.95 },
  {
    entity: 'PERSON',
    regex: new RegExp(`\\b(?:${NAMES.map((n) => n.replace(' ', '\\s+')).join('|')})\\b`, 'g'),
    detector: 'ner',
    confidence: 0.8,
  },
];

const encoder = new TextEncoder();
const bytes = (s: string) => encoder.encode(s).length;

/** The toy pipeline: find, drop overlaps (the first and longest wins), mask. */
/** The mock's run on a source: text only. */
function mockSource(source: Source): Redaction {
  switch (source.kind) {
    case 'text':
      return mockRedact(source.text);
    case 'file': {
      const text = FILES[source.path];
      if (text === undefined) throw new ApiError(404, `${source.path}: not found`);
      return mockRedact(text, source.path);
    }
    case 'upload': {
      let text: string;
      try {
        text = new TextDecoder('utf-8', { fatal: true }).decode(decodeBase64(source.data));
      } catch {
        throw new ApiError(422, `${source.name} is not UTF-8 text; the mock reads text only`);
      }
      return mockRedact(text, source.name);
    }
  }
}

/** The mock's explanation: one reading of the whole input, each finding kept. */
export function mockExplain(result: Redaction): Reading[] {
  const encoded = new TextEncoder().encode(result.input);
  const decode = (s: number, e: number) => new TextDecoder().decode(encoded.slice(s, e));
  return [
    {
      text: result.input,
      candidates: result.report.findings.map((f, i) => ({
        start: result.input_spans[i][0],
        end: result.input_spans[i][1],
        value: decode(result.input_spans[i][0], result.input_spans[i][1]),
        entity: f.entity,
        detector: f.detector,
        confidence: f.confidence,
        threshold: 0.7,
        verdict: 'kept' as const,
      })),
    },
  ];
}

export function mockRedact(input: string, name?: string): Redaction {
  const hits: { start: number; end: number; entity: string; detector: string; confidence: number }[] = [];
  for (const d of DETECTORS) {
    for (const m of input.matchAll(d.regex)) {
      hits.push({ start: m.index, end: m.index + m[0].length, entity: d.entity, detector: d.detector, confidence: d.confidence });
    }
  }
  hits.sort((a, b) => a.start - b.start || b.end - a.end);
  const kept: typeof hits = [];
  for (const h of hits) if (!kept.length || h.start >= kept[kept.length - 1].end) kept.push(h);

  const findings: Finding[] = [];
  const inputSpans: [number, number][] = [];
  const outputSpans: [number, number][] = [];
  let output = '';
  let at = 0;
  for (const h of kept) {
    output += input.slice(at, h.start);
    const before = input.slice(0, h.start);
    const lines = before.split('\n');
    const start = bytes(before);
    inputSpans.push([start, start + bytes(input.slice(h.start, h.end))]);
    findings.push({
      entity: h.entity,
      location: {
        kind: 'span',
        start,
        end: start + bytes(input.slice(h.start, h.end)),
        line: lines.length,
        column: [...lines[lines.length - 1]].length + 1,
      },
      confidence: h.confidence,
      detector: h.detector,
    });
    const replacement = `<${h.entity}>`;
    const outStart = bytes(output);
    output += replacement;
    outputSpans.push([outStart, outStart + bytes(replacement)]);
    at = h.end;
  }
  output += input.slice(at);
  return {
    name,
    input,
    output,
    input_spans: inputSpans,
    output_spans: outputSpans,
    report: {
      version: 1,
      findings,
      review_required: true,
      review_reasons: ['Mock: a toy detector over synthetic samples, not veilq.'],
    },
  };
}

function decodeBase64(data: string): Uint8Array {
  return Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
}

export function mockApi(): Api {
  return {
    mock: true,
    async info() {
      return { version: 'mock', root: '/mock', policy: 'built-in defaults', default_action: 'mask', actions: ['mask'], max_input: 32 << 20 };
    },
    async files(dir) {
      const prefix = dir ? `${dir}/` : '';
      const seen = new Map<string, Entry>();
      for (const [path, text] of Object.entries(FILES)) {
        if (!path.startsWith(prefix)) continue;
        const [head, ...tail] = path.slice(prefix.length).split('/');
        const isDir = tail.length > 0;
        seen.set(head, { name: head, path: prefix + head, dir: isDir, size: isDir ? 0 : bytes(text) });
      }
      if (dir && !seen.size) throw new ApiError(404, `${dir}: not found`);
      const entries = [...seen.values()].sort((a, b) => Number(b.dir) - Number(a.dir) || a.name.localeCompare(b.name));
      return { dir, entries, truncated: false };
    },
    async picture(path) {
      throw new ApiError(422, `${path}: the mock has no images`);
    },
    async setup() {
      return { available: false, exe: '/mock/veilq', install_dir: '/mock/bin', installed: false, runs_installed: false, start_menu: false, desktop: false, on_path: false };
    },
    async install() {
      throw new ApiError(422, 'the mock installs nothing');
    },
    async uninstall() {
      throw new ApiError(422, 'the mock uninstalls nothing');
    },
    async models() {
      return {
        dir: '/mock/models',
        components: ['ner', 'ocr', 'pdf'].map((name) => ({
          name,
          about: `Mock: ${name}`,
          installed: false,
          available: true,
          active: false,
          files: [],
        })),
      };
    },
    async fetchModel() {
      throw new ApiError(422, 'the mock downloads nothing');
    },
    async policy() {
      return {
        source: 'built-in defaults',
        user_file: '/mock/veilq.toml',
        user_in_use: true,
        policy: {
          version: 1,
          defaults: { action: 'mask', min_confidence: 0.7 },
          locales: ['de', 'en'],
          entities: {},
          patterns: [],
          allow: { values: [], patterns: [] },
        },
        entities: ['EMAIL_ADDRESS', 'IBAN_CODE', 'PERSON', 'PHONE_NUMBER'],
        locales: ['de', 'en'],
        actions: ['mask', 'pseudonymize', 'fake', 'remove'],
        validators: ['iban', 'luhn'],
      };
    },
    async tryPolicy() {
      return { toml: 'version = 1' };
    },
    async savePolicy() {
      throw new ApiError(422, 'the mock saves nothing');
    },
    async folders() {
      return { path: '', parent: null, folders: [] };
    },
    async setRoot() {
      throw new ApiError(422, 'the mock has one root');
    },
    async batch() {
      return null;
    },
    async startBatch() {
      throw new ApiError(422, 'the mock redacts no folders');
    },
    async cancelBatch() {},
    async checkUpdate() {
      return { current: 'mock', latest: 'mock', newer: false, url: 'https://github.com/nuetzliches/veilq/releases' };
    },
    async installUpdate() {
      throw new ApiError(422, 'the mock updates nothing');
    },
    async quit() {},
    async ping() {},
    async redact(source: Source, _action?: string, _unverified?: boolean, explain?: boolean) {
      const result = mockSource(source);
      if (explain) result.explain = mockExplain(result);
      return result;
    },
  };
}
