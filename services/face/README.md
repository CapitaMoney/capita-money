# capita-face – the face check

Is the person in front of the camera alive, and the person of the identity document? Runs in the identity service, keeps nothing. Decisions: [ADR 0017](../../docs/adr/0017-face-check.md).

```sh
./fetch-models.sh models                         # YuNet (MIT) + SFace (Apache 2.0), SHA-256 checked
cargo run --release -p capita-face -- models faces photo.jpg
cargo run --release -p capita-face -- models compare passport-photo.jpg selfie.jpg
cargo run --release -p capita-face -- models challenge
cargo run --release -p capita-face -- models check left,up passport-photo.jpg frame-*.jpg
cargo run --release -p capita-face -- models measure photos/   # one folder per person: the value for face dedup (ADR 0030)
CAPITA_FACE_MODELS=models cargo test --release -p capita-face
```

Frames for `check`: filmed with the camera as it sees (not mirrored): look straight, do the moves in order, look straight again; 4–60 pictures in time order.

`measure`: for face dedup (rule 33, ADR 0030) – how alike the photos of one person are, and those of different people; per value the share of missed doubles and the false holds per verification for groups of 1, 27 and 274 people. Use document-like photos taken years apart, of a set whose licence allows it.
