# 안내 음성 만들기 (MeloTTS)

칠판 타이머의 기본 안내 음성(`src/board/voice/*.ogg`)을 만드는 개발용 스크립트다.
앱 빌드·CI에서는 실행하지 않고, 만든 파일만 저장소에 넣는다. 문구는 `data/voice/cues.json`.

## 다시 만드는 방법

```bash
python3.11 -m venv venv && source venv/bin/activate
pip install torch==2.15.0 torchaudio==2.11.1 --index-url https://download.pytorch.org/whl/cpu
pip install -r scripts/tts/requirements.txt
pip install --no-deps git+https://github.com/myshell-ai/MeloTTS.git@209145371cff8fc3bd60d7be902ea69cbdb7965a
# MeloTTS는 일본어 모듈을 불러올 때 unidic 사전을 찾는다. 받지 않았다면 unidic_lite 사전으로 연결한다.
#   rm -rf venv/lib/python3.11/site-packages/unidic/dicdir
#   ln -s ../unidic_lite/dicdir venv/lib/python3.11/site-packages/unidic/dicdir
# g2p_en이 쓰는 nltk 자료(cmudict, averaged_perceptron_tagger_eng)가 필요하다: python -c "import nltk; nltk.download('cmudict'); nltk.download('averaged_perceptron_tagger_eng')"
python scripts/tts/generate.py
```

- 처음 실행할 때 Hugging Face에서 `myshell-ai/MeloTTS-Korean`(약 200MB)과 `kykim/bert-kor-base`를 내려받는다.
- 화자 `KR`, 속도 0.9. 16kHz 모노 Opus(24kbps, `.ogg`)로 줄인다. ffmpeg(libopus)가 없으면 WAV 그대로 저장한다.
- 숫자는 소리 내어 읽히도록 `ttsInput`에 한글(「일 분」「삼십 초」)로 넣는다.
- 2026-11 기준 파일 크기: timer-1min 5.6KB, timer-30s 6.1KB, timer-end 15.1KB.

## 사용한 것과 라이선스 (2026-11-06 확인)

| 구성 | 라이선스 | 비고 |
|---|---|---|
| MeloTTS 코드 (`myshell-ai/MeloTTS`) | MIT | 저장소 LICENSE |
| 한국어 모델 `myshell-ai/MeloTTS-Korean` | MIT | 모델 카드 `license: mit` |
| 한국어 BERT `kykim/bert-kor-base` | Apache-2.0로 표시 | 모델 카드에는 라이선스 표기가 없고, 제작자 저장소(kiyoungkim1/LM-kor) README의 License 항목이 Apache-2.0이다. 같은 README 머리말에 「모델의 상업적 사용의 경우 MOU를 통해 무료로 사용」이라는 안내가 있어 표기가 일치하지 않는다. 상업적 배포 전에 제작자에게 확인이 필요하다. |
| 한국어 G2P `g2pkk` | Apache-2.0 | |
| `jamo` | Apache-2.0 | |
| `python-mecab-ko` / `python-mecab-ko-dic` | BSD-3-Clause / Apache-2.0 | |
| `transformers` | Apache-2.0 | |
| `num2words` | LGPL | 개발 PC에서만 실행, 앱에 포함되지 않음 |
| `pykakasi` | GPLv3 | MeloTTS의 일본어 모듈이 불러옴. 개발 PC에서만 실행, 앱에 포함되지 않음 |

앱에 들어가는 것은 생성된 음성 파일뿐이며 위 도구·모델의 코드나 가중치는 앱에 들어가지 않는다.
고지 문구: 「안내 음성: MeloTTS(MIT)로 생성」.
