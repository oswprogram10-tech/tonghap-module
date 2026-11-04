// 메이킹북 인쇄 틀의 칸 배치. 단위는 모두 mm이고, 이 파일은 import 없이 Node 테스트에서 그대로 불러온다.

export type MakingbookTemplate = 'zine8' | 'card2' | 'accordion4';

export interface MakingbookSpec {
  template: MakingbookTemplate;
  title: string;
  pages: string[];
}

export type CellKind = 'cover' | 'page' | 'back';

export interface Cell {
  x: number;
  y: number;
  w: number;
  h: number;
  /** 칸 가운데를 기준으로 도는 각도(0 또는 180). */
  rotate: 0 | 180;
  kind: CellKind;
  /** 칸에 쓰는 안내 문구(표지는 비어 있을 수 있다). */
  text: string;
  /** 칸 번호(1부터, 접은 책에서의 쪽 순서). */
  index: number;
  /** 표지 제목을 쓰는 칸. */
  title: boolean;
  /** 그리는 쪽에서 채워 넣는 제목 글(스펙의 title). */
  titleText?: string;
  /** 「이름: ____」 칸. */
  name: boolean;
  /** 그림 그리는 네모 칸. */
  picture: boolean;
  /** 글쓰기 줄. */
  lines: boolean;
  /** 「날짜: ____」 칸. */
  date: boolean;
}

export interface FoldLine {
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  kind: 'fold' | 'cut';
}

export interface Layout {
  paper: { w: number; h: number; orientation: 'portrait' | 'landscape' };
  cells: Cell[];
  lines: FoldLine[];
}

export const A4 = { w: 210, h: 297 };
/** 안내 문구 14pt, 제목 24pt를 mm로(1pt = 25.4/72 mm). */
export const PT = 25.4 / 72;
export const GUIDE_MM = 14 * PT;
export const TITLE_MM = 24 * PT;

export const TEMPLATE_LABEL: Record<MakingbookTemplate, string> = {
  zine8: '8쪽 접이 책',
  card2: '반 접기 카드',
  accordion4: '4칸 병풍책',
};

export const TEMPLATE_PAGES: Record<MakingbookTemplate, number> = { zine8: 6, card2: 2, accordion4: 4 };

export const TEMPLATE_HELP: Record<MakingbookTemplate, string[]> = {
  zine8: ['표지와 뒷표지를 뺀 속 6쪽의 안내를 차례로 적어요'],
  card2: ['앞면 그림 안내', '안쪽 안내'],
  accordion4: ['1칸 안내', '2칸 안내', '3칸 안내', '4칸 안내'],
};

const blank = (): Omit<Cell, 'x' | 'y' | 'w' | 'h' | 'rotate' | 'kind' | 'text' | 'index'> => ({
  title: false,
  name: false,
  picture: false,
  lines: false,
  date: false,
});

export function layoutFor(spec: MakingbookSpec): Layout {
  const pages = spec.pages;
  if (spec.template === 'zine8') {
    // A4 가로 한 장을 2줄 x 4칸으로 쓴다(한 칸 74.25 x 105mm).
    const w = A4.h / 4;
    const h = A4.w / 2;
    // 접이 책 표준 배치: 위 줄은 거꾸로 5·4·3·2, 아래 줄은 6·7·8·1. 1쪽이 표지, 8쪽이 뒷표지.
    const order: { index: number; row: 0 | 1; col: number }[] = [
      { index: 5, row: 0, col: 0 },
      { index: 4, row: 0, col: 1 },
      { index: 3, row: 0, col: 2 },
      { index: 2, row: 0, col: 3 },
      { index: 6, row: 1, col: 0 },
      { index: 7, row: 1, col: 1 },
      { index: 8, row: 1, col: 2 },
      { index: 1, row: 1, col: 3 },
    ];
    const cells = order.map<Cell>(({ index, row, col }) => {
      const base = { x: col * w, y: row * h, w, h, rotate: (row === 0 ? 180 : 0) as 0 | 180, index, ...blank() };
      if (index === 1) return { ...base, kind: 'cover', text: '', title: true, name: true };
      if (index === 8) return { ...base, kind: 'back', text: '다 만들었어요!', date: true };
      return { ...base, kind: 'page', text: pages[index - 2] ?? '', picture: true };
    });
    const lines: FoldLine[] = [
      ...[1, 2, 3].map((i): FoldLine => ({ x1: i * w, y1: 0, x2: i * w, y2: A4.w, kind: 'fold' })),
      { x1: 0, y1: h, x2: w, y2: h, kind: 'fold' },
      { x1: w, y1: h, x2: 3 * w, y2: h, kind: 'cut' },
      { x1: 3 * w, y1: h, x2: A4.h, y2: h, kind: 'fold' },
    ];
    return { paper: { w: A4.h, h: A4.w, orientation: 'landscape' }, cells: cells.sort((a, b) => a.index - b.index), lines };
  }
  const land = { w: A4.h, h: A4.w };
  if (spec.template === 'card2') {
    const w = land.w / 2;
    const front: Cell = { x: 0, y: 0, w, h: land.h, rotate: 0, kind: 'cover', text: pages[0] ?? '', index: 1, ...blank(), title: true, picture: true };
    const inside: Cell = { x: w, y: 0, w, h: land.h, rotate: 0, kind: 'page', text: pages[1] ?? '', index: 2, ...blank(), lines: true };
    return { paper: { ...land, orientation: 'landscape' }, cells: [front, inside], lines: [{ x1: w, y1: 0, x2: w, y2: land.h, kind: 'fold' }] };
  }
  const w = land.w / 4;
  const cells = [0, 1, 2, 3].map<Cell>((i) => {
    const base = { x: i * w, y: 0, w, h: land.h, rotate: 0 as const, index: i + 1, text: pages[i] ?? '', ...blank(), picture: true };
    return i === 0 ? { ...base, kind: 'cover', title: true, name: true } : { ...base, kind: 'page' };
  });
  const lines = [1, 2, 3].map((i): FoldLine => ({ x1: i * w, y1: 0, x2: i * w, y2: land.h, kind: 'fold' }));
  return { paper: { ...land, orientation: 'landscape' }, cells, lines };
}

/** 글자 폭 추정(em): 한글·전각은 1, 그 밖은 0.55. */
function emWidth(ch: string): number {
  return /[ᄀ-ᇿ㄰-㆏가-힣　-〿＀-￯]/.test(ch) ? 1 : 0.55;
}

/** 폭(mm)과 글자 크기(mm)에 맞춰 줄을 나눈다. 공백에서 먼저 끊고, 안 되면 글자 사이에서 끊는다. */
export function wrapText(text: string, widthMm: number, fontMm: number): string[] {
  const max = widthMm / fontMm;
  const out: string[] = [];
  for (const para of text.split('\n')) {
    let line = '';
    let w = 0;
    for (const word of para.split(/(?<=\s)/)) {
      const ww = [...word].reduce((s, c) => s + emWidth(c), 0);
      if (w + ww <= max + 1e-9 || line === '') {
        if (w + ww > max + 1e-9) {
          // 낱말 하나가 한 줄보다 길면 글자 단위로 자른다.
          for (const ch of word) {
            const cw = emWidth(ch);
            if (w + cw > max + 1e-9 && line !== '') {
              out.push(line.trimEnd());
              line = '';
              w = 0;
            }
            line += ch;
            w += cw;
          }
        } else {
          line += word;
          w += ww;
        }
      } else {
        out.push(line.trimEnd());
        line = word;
        w = ww;
      }
    }
    out.push(line.trimEnd());
  }
  return out.filter((l, i, a) => l !== '' || i < a.length - 1 || a.length === 1);
}
