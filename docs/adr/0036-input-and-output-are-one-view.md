# ADR-0036: Input and output are one view

- **Status:** Accepted (card in the conversation, points 1 to 5, approved by the owner 2026-11-12)
- **Date:** 2026-11-12
- **Deciders:** project owner
- **Related:** [ADR-0033](0033-the-ui-is-one-working-surface-for-people-without-a-command-line.md), [ADR-0034](0034-scan-explains-what-it-read-and-why-it-decided.md)

## Context

Checking a redaction means looking at one place twice: what stood there in
the input, and what is left of it in the output. The two panes scrolled each
on its own, zoom existed only in the full-screen viewer, which shows one side
at a time, and the findings panel reacted to the pointer only. A user lined
the two sides up by hand, for every finding.

Lining them up by scroll offset fails for text: a placeholder is rarely as
long as the name it replaced, so lines wrap differently and the sides drift
apart further down a long text. For a picture the geometry is the same on
both sides, because a redaction paints over a region and keeps the size.

## Decision

- **One zoom, one place.** The input and the output share a zoom and the
  place they show; scrolling, dragging or zooming one side moves the other.
  There is no switch to uncouple them. The zoom controls sit in the input's
  header; Ctrl+wheel zooms both around the pointer. In the editor there is
  nothing to couple.
- **A place is a point in an anchor, not an offset.** Both sides render the
  same anchors: each page of a picture (the picture itself, not the pane
  around it), and each piece of a text, a finding or the plain text after
  one. The text between two findings is the same on both sides, so the same
  anchor shows the same words. A pane's middle names a point in an anchor;
  the other side shows that point at the same height.
- **Selecting a finding shows it on both sides.** A click in the list
  centres the finding in both panes; a click on a mark or a frame selects it
  in the list and lines the other side up with the clicked one; N and P step
  through the findings, Esc clears the selection. The selection does not
  change the output: Copy and Save … still take the whole of it.
- **A finding without a place says so.** A region inside a part (a picture
  in a document), on a PDF page past the rendered ones, or in a GIF's frames
  has no place in the panes; the list marks it "not in the preview" instead
  of a click that does nothing.
- **The viewer shares the view.** The full-screen viewer stays, with the
  panes' zoom: it opens on the place the pane showed, and the panes show the
  place it closed on.

## Alternatives considered

- **Couple by scroll fraction or pixels:** one line of code, and exact for
  pictures, but text drifts by a line per few findings; fixing it later
  would mean this design anyway.
- **A switch to uncouple the sides:** no use case came up in which looking
  at two different places of the same document helps a check; a switch can
  be added if one does.
- **Zoom only in the viewer, as before:** cheaper, but the viewer shows one
  side at a time, and the point of the panes is the comparison.

## Consequences

- Each piece of text is an element, so a text with many thousand findings
  renders tens of thousands of them. A scroll measures only a few: the
  anchor under the middle is found by a binary search (anchors are in
  document order, top to bottom), the other side's by its key; the other
  side follows once per frame (36,000 anchors: no measurable cost).
- An Office file has no view of its output, so there is nothing to couple
  and the list marks its findings as not in the preview, until the UI
  renders Office files.
- The anchor keys name findings by their position in the report, as the
  numbers in the list do (ADR-0033).

## Enforced by

- `ui/src/lib/view.ts` (anchors, mapping, coupling, which findings have a
  place) and its test `ui/src/lib/view.test.ts`
- `ui/src/App.vue` (the shared zoom, selection, N and P)
- `ui/src/components/ComparePane.vue`, `ImagePane.vue` (anchors, marks and
  frames that select), `FindingsPanel.vue` (selection, "not in the
  preview"), `ImageViewer.vue` (the shared zoom and place)

## What this ADR does not say

- Nothing about selecting several findings or acting on a selection
  (marking a gap, #18 and #34, needs its own card).
- The server's API is unchanged.
