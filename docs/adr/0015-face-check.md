# ADR 0015 – The face check

Status: accepted (task 1c, 2026-11-14). Built and tested: `services/face` (`capita-face`): face finding, cut-out, face print, comparison with the document photo, liveness by challenge. Not built: reading the document photo from the chip (task 1a; the photo comes in as a JPEG or PNG file until then), the HTTP interface in `capita-issuer`, the app side (filming).

## Context
A document check (2discuss F) proves that a document is genuine. It does not prove that the person sending it is the person on it: a stolen or borrowed passport would do. The usual answer: compare a selfie with the photo on the chip, and make sure the selfie is of a living person in front of the camera, not a photo of a photo.

## Decision

### Where it runs: in the identity service (owner 2026-11-14)
Not on the phone. A changed app could simply answer "same person"; only a check the person cannot change counts. The app sends the frames; the service decides.

### Nothing is kept
Pictures and face prints exist in memory for the length of one call, then are dropped. Not stored, not logged, not sent on. What remains is the outcome inside the claim (ADR 0014): verified or not. No face print goes on chain.

### Models: OpenCV Zoo, open licences
- Finding faces: YuNet (`face_detection_yunet_2023mar.onnx`, MIT licence) – box, score, five landmarks.
- Telling faces apart: SFace (`face_recognition_sface_2021dec.onnx`, Apache 2.0) – 128 numbers per face.
- Run by `tract` (pure Rust): no C library, no Python on the server.
- Not in the repository (38 MB): `services/face/fetch-models.sh` downloads them and checks their SHA-256.
- Checked against OpenCV's own implementation: same boxes and landmarks, same cut-out (pixel difference ≤ 1), same face print to 4 decimal places.

### Same person: cosine ≥ 0.363
The value published with SFace for its test sets. Both straight looks of the take (start and end) must reach it; the weaker counts. Technical value in `Settings`, not a rule. Vermutung (Claude, ungeprüft): document photos are small and older; the rate of honest people refused must be measured with real people before the value is final.

### Alive: a challenge of head movements (owner 2026-11-14)
1. The service names two random movements (left, right, up, down; never the same twice in a row), valid 60 s, usable once.
2. The app films: straight – movement – movement – straight; 4 to 60 frames.
3. The service checks every frame: exactly one face, not too small (≥ 30 px between the eyes); from frame to frame the face does not jump (≤ 1.5 eye distances) or change size (≤ × 1.5) – one take, not a cut.
4. The pose comes from the landmarks alone: turn = sideways offset of the nose tip from the middle of the eyes; nod = height of the nose between eyes and mouth against the first straight look. The asked movements must appear in order between a straight look at the start and one at the end.
5. Both straight looks are compared with the document photo.

Frames are as the camera sees them, not mirrored. A still photo, a printed photo or a picture on a screen cannot turn its head and fails.

### Not high-risk AI
EU AI Act, Annex III 1(a): biometric verification whose sole purpose is to confirm that a person is who they claim to be is not a high-risk system (one-to-one comparison, no search in a database). The check compares one person with one document and searches nothing.

## Consequences
+ A stolen document alone no longer gets an account.
+ Open, auditable, runs on a plain server; no provider, no fee per check.
+ Same code for any document source: chip (1a), EUDI wallet photo, later others.
− The challenge stops photos and replayed videos of other movements; it does not stop a video made for the challenge in real time (deepfake) fed into a fake camera. Against that: the app on a genuine phone (attestation of the device, later) or a second, passive model that tells skin from screen. Offene Frage: needed before real money?
− Face prints are biometric data, GDPR Art. 9: explicit consent of the person before the check, and a data protection impact assessment (Art. 35) before real persons. Vermutung (Claude, ungeprüft): processing in memory without storage keeps the duty small; legal view needed (P04.3).
− The models are trained elsewhere; how well they serve every skin tone and age is not known here. Measure with real people before go-live.

## Try it
```sh
services/face/fetch-models.sh models
cargo run --release -p capita-face -- models compare passport-photo.jpg selfie.jpg
cargo run --release -p capita-face -- models challenge
cargo run --release -p capita-face -- models check left,up passport-photo.jpg frame-*.jpg
```
Your own pictures stay on your computer; the program prints a result and keeps nothing.
