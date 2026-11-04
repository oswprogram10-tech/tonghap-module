# 자막 정확도 측정 도구 (개발자용)

앱의 자막(whisper.cpp)이 아이 음성을 얼마나 틀리는지 재는 도구다. 앱에는 들어가지 않는다. 설계: `docs/design/17-stt-accuracy-eval.md`.

## 녹음 모으는 법

- 보호자 동의를 받고 `manifest.json`의 `consent`에 기록한다.
- 아이가 이름·학교 같은 개인 정보를 말하지 않게 한다. 말했다면 그 녹음은 쓰지 않는다.
- 교실 소음은 그대로 둔다(조용한 방에서만 재면 실제보다 좋게 나온다). 한 녹음은 10~30초.
- 형식은 앱 녹음과 같은 16kHz 모노 16비트 WAV. 다르면 도구가 오류로 알린다.
- 기준 문장(`text`)은 사람이 들으며 받아 적는다.

## 묶음 만드는 법

폴더에 `manifest.json`과 wav를 둔다(형식은 `manifest.schema.json`, 예시는 `example/`).
`speaker`는 `child-1` 같은 익명 기호만 쓴다(한글 2~4자 값은 이름으로 보고 경고한다). `tags`로 `교실소음` 등을 붙이면 태그별 CER가 나온다.
실제 묶음은 `ml/stt/data/` 아래 두면 `.gitignore`가 막아 준다. 아이 음성을 저장소에 올리지 않는다.

## 실행

```bash
# 검사만(whisper 불필요)
node ml/stt/eval.mjs --set ml/stt/example --dry-run

# 측정(모델 여러 개를 이어서 비교 가능)
node ml/stt/eval.mjs --set ml/stt/data/round1 --whisper <whisper-cli 경로> \
  --model <ggml-small-q5_1.bin> --model <다른 모델> --threads 4 --out ml/out/stt
```

결과는 `report-<시각>.md`와 `.json`: 모델별 CER·WER·평균 배율(실행 시간 / 음성 길이), 태그별 CER, 가장 틀린 10개 항목.
`--max-cer 0.5`를 주면 CER가 넘을 때 실패한다(CI 스모크용).
whisper 인자는 `whisper-args.json`에서 읽는다. 앱의 `WHISPER_ARGS`와 Rust 테스트로 같게 유지한다. 바꾸면 `TONGHAP_WRITE_WHISPER_ARGS=1 cargo test --manifest-path src-tauri/Cargo.toml whisper_args_json`으로 다시 쓴다.
인자에 한국어를 넣지 않는다(whisper-cli는 윈도우에서 인자를 CP949로 받는다).

## 지표

- CER(주 지표): NFC, 소문자, 문장부호·공백 제거 후 글자 단위 편집 거리 / 기준 글자 수. 한국어는 띄어쓰기가 흔들려 CER를 본다.
- WER(참고): 공백 기준 낱말 단위.

## 미세조정 판단 기준 (첫 측정 뒤 조정)

아이 음성에서 small 모델의 CER가 15% 이하면 미세조정을 보류하고, 25%를 넘으면 미세조정을 검토한다. 그 사이는 태그별·항목별 오류를 보고 정한다.
