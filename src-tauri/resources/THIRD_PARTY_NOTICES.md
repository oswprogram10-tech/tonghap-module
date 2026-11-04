# 제3자 구성 요소 고지

이 앱은 다음 구성 요소를 포함하거나, 처음 실행할 때 내려받습니다.

| 구성 요소 | 용도 | 라이선스 | 출처 |
|---|---|---|---|
| llama.cpp (llama-bench, llama-server, ggml) | 언어 모델 속도 측정, 평어 초안 생성(이 PC 안에서만 실행) | MIT | https://github.com/ggml-org/llama.cpp |
| whisper.cpp (whisper-cli, ggml) | 음성 인식 속도 측정 | MIT | https://github.com/ggml-org/whisper.cpp |
| samples/jfk.wav | 음성 인식 측정용 샘플(11초) | whisper.cpp 저장소에 포함된 파일 | https://github.com/ggml-org/whisper.cpp/tree/master/samples |
| Qwen2.5 0.5B/1.5B Instruct GGUF (내려받기) | 언어 모델 측정 | Apache-2.0 | https://huggingface.co/Qwen |
| Qwen3 4B GGUF Q4_K_M (내려받기) | 평어 초안 생성 | Apache-2.0 | https://huggingface.co/Qwen/Qwen3-4B-GGUF |
| Whisper base ggml (내려받기) | 음성 인식 측정 | MIT | https://huggingface.co/ggerganov/whisper.cpp |
| Whisper small q5_1 ggml (내려받기) | 음성 메모 자동 자막(기본) | MIT | https://huggingface.co/ggerganov/whisper.cpp |
| Whisper base q5_1 ggml (내려받기) | 음성 메모 자동 자막(느린 PC용) | MIT | https://huggingface.co/ggerganov/whisper.cpp |

## llama.cpp, whisper.cpp 라이선스 전문

```
MIT License

Copyright (c) 2023-2026 The ggml authors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
