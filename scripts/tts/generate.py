"""칠판 타이머 안내 음성을 MeloTTS로 만든다(개발할 때 한 번, 결과 파일만 커밋).

사용: python scripts/tts/generate.py [--out src/board/voice] [--keep-wav 폴더]
앱 빌드·CI에서는 실행하지 않는다. 자세한 내용은 scripts/tts/README.md.
"""
import argparse
import json
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(ROOT / "src/board/voice"))
    ap.add_argument("--keep-wav", default=None, help="원본 WAV를 이 폴더에도 남긴다")
    ap.add_argument("--speed", type=float, default=0.9)
    args = ap.parse_args()

    from melo.api import TTS  # 무거우므로 인자 처리 뒤에 불러온다

    cues = json.loads((ROOT / "data/voice/cues.json").read_text(encoding="utf-8"))["cues"]
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    ffmpeg = shutil.which("ffmpeg")
    model = TTS(language="KR", device="cpu")
    speaker = model.hps.data.spk2id["KR"]

    with tempfile.TemporaryDirectory() as tmp:
        for cue in cues:
            wav = Path(tmp) / f"{cue['id']}.wav"
            model.tts_to_file(cue["ttsInput"], speaker, str(wav), speed=args.speed, quiet=True)
            if args.keep_wav:
                Path(args.keep_wav).mkdir(parents=True, exist_ok=True)
                shutil.copy(wav, Path(args.keep_wav) / wav.name)
            if ffmpeg:
                dest = out / f"{cue['id']}.ogg"
                subprocess.run(
                    [ffmpeg, "-y", "-loglevel", "error", "-i", str(wav), "-ar", "16000", "-ac", "1",
                     "-c:a", "libopus", "-b:a", "24k", str(dest)],
                    check=True,
                )
            else:
                dest = out / f"{cue['id']}.wav"
                shutil.copy(wav, dest)
            print(f"{cue['id']}: {dest} ({dest.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
