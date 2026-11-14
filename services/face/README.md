# capita-face – the face check

Is the person in front of the camera alive, and the person of the identity document? Runs in the identity service, keeps nothing. Decisions: [ADR 0015](../../docs/adr/0015-face-check.md).

```sh
./fetch-models.sh models                         # YuNet (MIT) + SFace (Apache 2.0), SHA-256 checked
cargo run --release -p capita-face -- models faces photo.jpg
cargo run --release -p capita-face -- models compare passport-photo.jpg selfie.jpg
cargo run --release -p capita-face -- models challenge
cargo run --release -p capita-face -- models check left,up passport-photo.jpg frame-*.jpg
CAPITA_FACE_MODELS=models cargo test --release -p capita-face
```

Frames for `check`: filmed with the camera as it sees (not mirrored): look straight, do the moves in order, look straight again; 4–60 pictures in time order.
