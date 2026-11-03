import { toPng } from 'html-to-image';
import { SHEET_HEIGHT, SHEET_WIDTH } from './api';

/** 두 프레임을 기다려 React가 그린 결과가 DOM에 반영되게 한다. */
export const nextPaint = () => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())));

async function imagesReady(node: HTMLElement) {
  const imgs = Array.from(node.querySelectorAll('img'));
  await Promise.all(
    imgs.map((img) =>
      img.complete
        ? Promise.resolve()
        : new Promise<void>((res) => {
            img.onload = () => res();
            img.onerror = () => res();
          }),
    ),
  );
}

/** 리포트 노드를 1240×1754 PNG 바이트로 만든다. 외부 글꼴·주소는 쓰지 않는다. */
export async function renderPng(node: HTMLElement): Promise<Uint8Array> {
  await imagesReady(node);
  const url = await toPng(node, { width: SHEET_WIDTH, height: SHEET_HEIGHT, pixelRatio: 1, skipFonts: true, backgroundColor: '#ffffff' });
  const b64 = url.slice(url.indexOf(',') + 1);
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

const INVALID = /[\\/:*?"<>|\u0000-\u001f]/g;

/** `<번호>-<이름>-<주제책>-리포트.png` */
export function reportFileName(number: number, name: string, topic: string): string {
  const parts = [String(number), name, topic, '리포트'].filter(Boolean).join('-');
  return `${parts.replace(INVALID, '_').replace(/\s+/g, '_')}.png`;
}

export function joinPath(dir: string, file: string): string {
  const sep = dir.includes('\\') ? '\\' : '/';
  return dir.endsWith(sep) ? `${dir}${file}` : `${dir}${sep}${file}`;
}
