// 앱이 만드는 진단 파일(압축 없는 zip)을 Node만으로 읽는다. 중앙 디렉터리의 이름 목록과 내용을 돌려준다.
const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
export const crc32 = (buf) => {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};

/** @param {Buffer} buf @returns {{name: string, data: Buffer}[]} 압축된 항목이나 깨진 zip이면 한국어 메시지로 던진다. */
export function readZipEntries(buf) {
  let eocd = -1;
  for (let i = buf.length - 22; i >= Math.max(0, buf.length - 22 - 65535); i--) {
    if (buf.readUInt32LE(i) === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) throw new Error('zip 파일이 아닙니다(중앙 디렉터리 끝 표시를 찾지 못함)');
  const count = buf.readUInt16LE(eocd + 10);
  let p = buf.readUInt32LE(eocd + 16);
  const out = [];
  for (let i = 0; i < count; i++) {
    if (buf.readUInt32LE(p) !== 0x02014b50) throw new Error(`zip 중앙 디렉터리 ${i + 1}번째 항목이 깨졌습니다`);
    const method = buf.readUInt16LE(p + 10);
    const crc = buf.readUInt32LE(p + 16);
    const size = buf.readUInt32LE(p + 24);
    const nameLen = buf.readUInt16LE(p + 28);
    const extraLen = buf.readUInt16LE(p + 30);
    const commentLen = buf.readUInt16LE(p + 32);
    const local = buf.readUInt32LE(p + 42);
    const name = buf.subarray(p + 46, p + 46 + nameLen).toString('utf8');
    if (method !== 0) throw new Error(`압축된 항목입니다(${name}, 방식 ${method}). 진단 파일은 압축 없는 zip이어야 합니다`);
    if (buf.readUInt32LE(local) !== 0x04034b50) throw new Error(`zip 로컬 헤더가 깨졌습니다: ${name}`);
    const start = local + 30 + buf.readUInt16LE(local + 26) + buf.readUInt16LE(local + 28);
    const data = Buffer.from(buf.subarray(start, start + size));
    if (crc32(data) !== crc) throw new Error(`zip 항목의 CRC가 맞지 않습니다: ${name}`);
    out.push({ name, data });
    p += 46 + nameLen + extraLen + commentLen;
  }
  return out;
}
