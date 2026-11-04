import { useEffect } from 'react';
import { createPortal } from 'react-dom';
import { GUIDE_MM, TEMPLATE_LABEL, TITLE_MM, layoutFor, wrapText, type Cell, type Layout, type MakingbookSpec } from './layouts';

const FONT = '"Malgun Gothic", "맑은 고딕", "Apple SD Gothic Neo", sans-serif';
const INK = '#222';
const PAD = 5;
const GUIDE_LEAD = GUIDE_MM * 1.45;
const TITLE_LEAD = TITLE_MM * 1.3;

/** 칸 하나를 칸 왼쪽 위를 (0,0)으로 그린다. */
function CellArt({ cell }: { cell: Cell }) {
  const { w, h } = cell;
  const inner = w - PAD * 2;
  const nodes: React.ReactNode[] = [];
  let y = PAD;
  const text = (key: string, lines: string[], size: number, lead: number, anchor: 'start' | 'middle' = 'start') => {
    const x = anchor === 'start' ? PAD : w / 2;
    lines.forEach((l, i) => {
      nodes.push(
        <text key={`${key}${i}`} x={x} y={y + size + i * lead} fontSize={size} textAnchor={anchor} fill={INK} fontFamily={FONT}>
          {l}
        </text>,
      );
    });
    y += size + (lines.length - 1) * lead + size * 0.5;
  };
  const nameRow = (at: number) => {
    nodes.push(
      <text key="name" x={PAD} y={at} fontSize={GUIDE_MM} fill={INK} fontFamily={FONT}>
        이름:
      </text>,
      <line key="nameline" x1={PAD + GUIDE_MM * 2.6} y1={at} x2={w - PAD} y2={at} stroke={INK} strokeWidth={0.35} />,
    );
  };

  if (cell.kind === 'back') {
    y = h * 0.3;
    text('back', wrapText(cell.text, inner, TITLE_MM), TITLE_MM, TITLE_LEAD, 'middle');
    if (cell.date) {
      const at = h * 0.62;
      nodes.push(
        <text key="date" x={PAD} y={at} fontSize={GUIDE_MM} fill={INK} fontFamily={FONT}>
          날짜:
        </text>,
        <line key="dateline" x1={PAD + GUIDE_MM * 2.6} y1={at} x2={w - PAD} y2={at} stroke={INK} strokeWidth={0.35} />,
      );
    }
    return <>{nodes}</>;
  }
  if (cell.title) {
    // 제목은 tspan 없이 줄마다 따로 그린다. 표지는 가운데, 나머지는 왼쪽.
    y = cell.picture ? PAD : h * 0.18;
    text('title', wrapText(cell.titleText ?? '', inner, TITLE_MM), TITLE_MM, TITLE_LEAD, cell.picture ? 'start' : 'middle');
    y += 3;
  }
  if (cell.name) {
    if (cell.picture) {
      nameRow(y + GUIDE_MM + 2);
      y += GUIDE_MM + 10;
    } else {
      nameRow(h - 18);
    }
  }
  if (cell.text) text('guide', wrapText(cell.text, inner, GUIDE_MM), GUIDE_MM, GUIDE_LEAD);
  if (cell.picture) {
    y += 2;
    nodes.push(<rect key="pic" x={PAD} y={y} width={inner} height={Math.max(10, h - PAD - y)} fill="none" stroke={INK} strokeWidth={0.4} rx={1.5} />);
  }
  if (cell.lines) {
    y += 4;
    for (let ly = y + 10; ly < h - PAD; ly += 11) {
      nodes.push(<line key={`l${ly}`} x1={PAD} y1={ly} x2={w - PAD} y2={ly} stroke={INK} strokeWidth={0.3} />);
    }
  }
  return <>{nodes}</>;
}

export function MakingbookSheet({ layout, spec }: { layout: Layout; spec: MakingbookSpec }) {
  const { paper } = layout;
  return (
    <svg
      viewBox={`0 0 ${paper.w} ${paper.h}`}
      xmlns="http://www.w3.org/2000/svg"
      role="img"
      aria-label={`${spec.title} ${TEMPLATE_LABEL[spec.template]} 인쇄 틀`}
      style={{ ['--sheet-w' as string]: `${paper.w}mm`, ['--sheet-h' as string]: `${paper.h - 0.5}mm`, aspectRatio: `${paper.w} / ${paper.h}` }}
    >
      <rect x={0} y={0} width={paper.w} height={paper.h} fill="#fff" />
      {layout.cells.map((c) => (
        <g key={c.index} transform={`translate(${c.x} ${c.y})${c.rotate ? ` rotate(${c.rotate} ${c.w / 2} ${c.h / 2})` : ''}`}>
          <CellArt cell={{ ...c, titleText: spec.title } as Cell} />
        </g>
      ))}
      {layout.lines.map((l, i) => (
        <line
          key={i}
          x1={l.x1}
          y1={l.y1}
          x2={l.x2}
          y2={l.y2}
          stroke={l.kind === 'cut' ? '#111' : '#555'}
          strokeWidth={l.kind === 'cut' ? 0.6 : 0.35}
          strokeDasharray={l.kind === 'cut' ? undefined : '2.5 2'}
        />
      ))}
      {layout.lines
        .filter((l) => l.kind === 'cut')
        .map((l, i) => (
          <text key={`s${i}`} x={l.x1 + 0.8} y={l.y1 + 4.2} fontSize={5} fill="#111" fontFamily={'"Segoe UI Symbol", "Malgun Gothic", sans-serif'}>
            ✂
          </text>
        ))}
    </svg>
  );
}

/** 접는 순서 그림(인쇄되지 않음). */
function FoldSteps() {
  const sheet = <rect x={2} y={13} width={40} height={28} fill="#fff" stroke={INK} strokeWidth={1.2} />;
  const dash = { stroke: '#555', strokeWidth: 1, strokeDasharray: '3 2' } as const;
  const steps: { text: string; art: React.ReactNode }[] = [
    {
      text: '종이를 가로로 반 접었다 펴요',
      art: (
        <>
          {sheet}
          <line x1={2} y1={27} x2={42} y2={27} {...dash} />
        </>
      ),
    },
    {
      text: '세로로 반, 또 반 접었다 펴요. 접는 선이 8칸이 돼요',
      art: (
        <>
          {sheet}
          <line x1={12} y1={13} x2={12} y2={41} {...dash} />
          <line x1={22} y1={13} x2={22} y2={41} {...dash} />
          <line x1={32} y1={13} x2={32} y2={41} {...dash} />
          <line x1={2} y1={27} x2={42} y2={27} {...dash} />
        </>
      ),
    },
    {
      text: '가위로 가운데 두 칸 사이 선만 잘라요',
      art: (
        <>
          {sheet}
          <line x1={12} y1={27} x2={32} y2={27} stroke="#111" strokeWidth={2} />
          <text x={2} y={33} fontSize={10} fill="#111">
            ✂
          </text>
        </>
      ),
    },
    {
      text: '가로로 접고, 잘린 곳을 벌려 십자 모양으로 모은 뒤 책으로 접어요',
      art: (
        <>
          <rect x={8} y={20} width={28} height={12} fill="#fff" stroke={INK} strokeWidth={1.2} />
          <rect x={17} y={9} width={10} height={34} fill="#fff" stroke={INK} strokeWidth={1.2} />
        </>
      ),
    },
  ];
  return (
    <ol className="mb-steps mb-noprint" aria-label="접는 순서">
      {steps.map((s, i) => (
        <li key={i}>
          <svg viewBox="0 0 44 54" width={64} height={78} aria-hidden="true">
            {s.art}
          </svg>
          <span>
            <b>{i + 1}.</b> {s.text}
          </span>
        </li>
      ))}
    </ol>
  );
}

/** 메이킹북 인쇄 틀 미리보기 창. 인쇄하면 틀만 종이에 나온다. */
export default function MakingbookPrint({ spec, onClose }: { spec: MakingbookSpec; onClose: () => void }) {
  const layout = layoutFor(spec);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return createPortal(
    <div className="mb-modal" role="dialog" aria-modal="true" aria-label="메이킹북 틀 인쇄 미리보기">
      <style>{`@page { size: A4 ${layout.paper.orientation}; margin: 0 }`}</style>
      <div className="mb-body">
        <div className="rec-row rec-wrap mb-noprint">
          <h2 className="rec-grow">
            {spec.title} · {TEMPLATE_LABEL[spec.template]}
          </h2>
          <button className="diag-primary" onClick={() => window.print()}>
            인쇄
          </button>
          <button className="diag-secondary" onClick={onClose}>
            닫기
          </button>
        </div>
        <p className="diag-hint mb-noprint">
          흑백으로 인쇄해도 됩니다. 이름과 날짜는 아이가 직접 씁니다.
          {spec.template === 'card2' && ' 가운데 점선을 따라 반으로 접어요. 왼쪽이 앞면, 오른쪽이 안쪽이에요.'}
          {spec.template === 'accordion4' && ' 점선을 따라 병풍처럼 접어요.'}
        </p>
        <div className="mb-sheet">
          <MakingbookSheet layout={layout} spec={spec} />
        </div>
        {spec.template === 'zine8' && <FoldSteps />}
      </div>
    </div>,
    document.body,
  );
}
