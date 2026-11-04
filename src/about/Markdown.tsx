import type { ReactNode } from 'react';

// 고지문(THIRD_PARTY_NOTICES.md)을 보여 줄 만큼만 지원하는 간단한 마크다운: 제목, 표, 코드 블록, 문단.

type Block =
  | { t: 'h'; level: number; text: string }
  | { t: 'code'; text: string }
  | { t: 'table'; head: string[]; rows: string[][] }
  | { t: 'p'; text: string };

const cells = (line: string) =>
  line
    .trim()
    .replace(/^\|/, '')
    .replace(/\|$/, '')
    .split('|')
    .map((c) => c.trim());

const isSeparator = (line: string) => /^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)*\|?\s*$/.test(line);

export function parseMarkdown(src: string): Block[] {
  const lines = src.replace(/\r\n/g, '\n').split('\n');
  const out: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.startsWith('```')) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i].startsWith('```')) body.push(lines[i++]);
      i++;
      out.push({ t: 'code', text: body.join('\n') });
    } else if (/^#{1,6}\s/.test(line)) {
      const m = /^(#+)\s+(.*)$/.exec(line)!;
      out.push({ t: 'h', level: m[1].length, text: m[2] });
      i++;
    } else if (line.trim().startsWith('|') && i + 1 < lines.length && isSeparator(lines[i + 1])) {
      const head = cells(line);
      i += 2;
      const rows: string[][] = [];
      while (i < lines.length && lines[i].trim().startsWith('|')) rows.push(cells(lines[i++]));
      out.push({ t: 'table', head, rows });
    } else if (line.trim() === '') {
      i++;
    } else {
      const para: string[] = [line];
      i++;
      while (i < lines.length && lines[i].trim() !== '' && !/^(#{1,6}\s|```|\|)/.test(lines[i])) para.push(lines[i++]);
      out.push({ t: 'p', text: para.join(' ') });
    }
  }
  return out;
}

export default function Markdown({ source }: { source: string }) {
  const blocks = parseMarkdown(source);
  return (
    <div className="md">
      {blocks.map((b, i): ReactNode => {
        switch (b.t) {
          case 'h': {
            // 화면 제목 아래에 놓이므로 한 단계 낮춘다(최소 h4).
            const Tag = `h${Math.min(6, Math.max(4, b.level + 2))}` as 'h4' | 'h5' | 'h6';
            return <Tag key={i}>{b.text}</Tag>;
          }
          case 'code':
            return (
              <pre key={i} tabIndex={0}>
                {b.text}
              </pre>
            );
          case 'table':
            return (
              <div className="md-table" key={i} tabIndex={0}>
                <table>
                  <thead>
                    <tr>
                      {b.head.map((h, j) => (
                        <th key={j}>{h}</th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {b.rows.map((r, j) => (
                      <tr key={j}>
                        {r.map((c, k) => (
                          <td key={k}>{c}</td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            );
          default:
            return <p key={i}>{b.text}</p>;
        }
      })}
    </div>
  );
}
