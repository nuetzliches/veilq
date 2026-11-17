# A learned decider's models, measured

- **Issue:** #95, part of #35
- **Measured:** 2026-11-17, on veilq `ed3320f` (corpus with minutes, #97)
- **Decision it feeds:** the review card of #35

This page stands on its own: the numbers, how they were made and what
follows. The scripts that made them live in a separate repository of the
measurements; everything they read is veilq's synthetic corpus, made again
from its seeds.

## The question

#35 proposes a learned decider between the detectors and the policy. Before
one is built, two models are chosen by measurement: the **head** that
decides a candidate, and the **encoder** that may give it a vector of the
words around the candidate. The line both have to beat is today's decision:
thresholds and hand rules.

## How it was measured

- **Data:** `veilq-eval candidates` (#96) on corpora of seeds 1 to 20 with
  NER: every candidate the detectors proposed, kept or not, labelled
  against the truth by value. 14 991 candidates, 1 267 of them not personal
  data. The corpus is built for recall; the minutes (#97) add hard negatives
  (a ticket number written like a phone number, an order number that starts
  like an international one, a build version, a meeting date beside a
  name).
- **Split:** leave one template family out (`de/text/letter`,
  `en/csv/customers`, ...; ten families): each family is the test once,
  trained on the other nine. A split by seed alone would measure how well a
  model learns the generator.
- **Fail closed:** a head's threshold is set on its training data so that it
  keeps at least 99.5 % of the personal data there. **Veto** is today's
  decision, dropped only where the head is surer than it ever was wrong in
  training.
- **Counted** per candidate (a decider decides candidates), not per truth
  value; `veilq-eval check` measures the latter.
- **Encoders** read the value with up to 100 characters around it, mean
  pooled over the value's tokens. A logistic head takes the scaled vector,
  a gradient-boosting head its first 32 principal components; both beside
  the named features (confidence, margin to the threshold, detector, type,
  format, the value's shape, the words before and after, the column's
  name).

## Results

| Head | Encoder (licence, size) | recall | precision | missed | extra |
|---|---|---|---|---|---|
| today's rules | – | 1.000 | 0.977 | 0 | 327 |
| logistic | – | 0.947 | 0.986 | 722 | 187 |
| boosting | – | 0.991 | 0.990 | 129 | 142 |
| boosting, veto | – | 0.996 | 0.989 | 50 | 150 |
| boosting, veto | multilingual-e5-small (MIT, 118 M) | 0.998 | 0.992 | 21 | 116 |
| logistic, veto | multilingual-e5-small | 0.981 | 0.998 | 266 | 31 |
| boosting, veto | paraphrase-multilingual-MiniLM-L12 (Apache-2.0, 118 M) | 0.996 | 0.990 | 55 | 142 |
| logistic, veto | paraphrase-multilingual-MiniLM-L12 | 0.979 | 0.999 | 289 | 8 |
| boosting, veto | mdeberta-v3-base (MIT, 280 M) | 0.999 | 0.991 | 16 | 129 |
| boosting, veto | GLiNER's encoder, as veilq ships it (Apache-2.0) | 0.994 | 0.995 | 89 | 71 |

Time per candidate on one 8-core desktop CPU (Ryzen 7 7700X): e5-small 15 ms, MiniLM 18 ms,
GLiNER's encoder 25 ms (mdeberta, the same architecture, measured slower in
this run for a reason in the tooling, not the model). Training a boosting
head on the 13 000 candidates of nine families: 2 to 5 seconds, without
and with a vector.

## What follows

1. **No head and no encoder reaches today's recall on a template it has not
   seen**, even when it may only veto the rules: the best still misses 16
   values the rules catch. A learned decider therefore **never drops what a
   rule keeps**.
2. **Vectors cut the extra boxes** (327 to 8 with MiniLM), at a recall a
   decider may not have. Where that helps without the risk is **review**:
   ordering what a person checks first, and lifting candidates below their
   threshold into review instead of passing them over.
3. **The encoder veilq ships already is not the better one here.** Reusing
   GLiNER's encoder saves a download, not quality.
4. **Candidate to build on:** multilingual-e5-small (MIT, 384 dimensions,
   15 ms per candidate on a CPU) with a gradient-boosting head on named
   features. Its ONNX export and its weights' training data are checked in
   #35's review card before it is pinned (ADR-0022).

## Limits

- Ten template families are few, and the hard negatives are of six kinds;
  a model that holds here may still fail on a real document's layout.
- The label joins a candidate to the truth by value (the readings hold no
  document locations): a value that matches by chance counts as personal
  data.
- Pictures and containers are not in the data yet (their truth has no
  value to join on).
- Synthetic only, by rule: no real document was measured.
