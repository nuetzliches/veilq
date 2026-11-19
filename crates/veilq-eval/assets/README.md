# Assets of the corpus generator

`NotoSans-Regular.ttf`: Noto Sans 2.015 (hinted TTF), from
`notofonts/notofonts.github.io` at commit
`4cf78353cd1668040ba8efa13029cd910fcdf9e2`
(`fonts/NotoSans/hinted/ttf/NotoSans-Regular.ttf`), SHA-256
`478c558ea716033cd60c03438f628dfa75694dcf6b5f6d505a2f05fd2b4f3823`.
Licensed under the SIL Open Font License 1.1 (`NotoSans-OFL.txt`). The
generator draws the text of its synthetic pictures with it (ADR-0023); it is
not part of veilq.

`IndieFlower-Regular.ttf`: Indie Flower, from `google/fonts` at commit
`82823f5bc3fb17ba48b58c0b66dadfe4bb546fa9`
(`ofl/indieflower/IndieFlower-Regular.ttf`), SHA-256
`ccc94b22b156e9c5dfe50fd051f01b097600b252c24473e624bb43a143140a94`.
Licensed under the SIL Open Font License 1.1 (`IndieFlower-OFL.txt`). The
generator writes the hand-filled values of its synthetic forms with it
(#59); it is not part of veilq. Of the hands tried (Caveat, Indie Flower,
Nothing You Could Do, Reenie Beanie, Shadows Into Light), OCR read Caveat
without a fault, and the others split a value into lines of their own,
which no second reading mends; Indie Flower makes OCR misread letters
inside a line, which is what the handwriting recogniser is for.
