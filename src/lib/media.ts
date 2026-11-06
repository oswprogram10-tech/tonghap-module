/**
 * 사진·음성의 화면 주소. 파일 경로가 아니라 **첨부 id**로 `tgmedia` 프로토콜에 요청한다(설계 21).
 * 앱이 첨부 id로 파일을 찾아 복호화해 돌려주므로, 디스크의 파일은 암호문이어도 화면에서는 그대로 보인다.
 * 주소 형태는 Tauri가 정한 규칙을 따른다: 윈도우·안드로이드는 `http://tgmedia.localhost/<id>`, 그 밖은 `tgmedia://localhost/<id>`.
 */
export const MEDIA_SCHEME = 'tgmedia';

export function mediaUrlFor(id: string, userAgent: string): string {
  const key = encodeURIComponent(id);
  return /Windows|Android/i.test(userAgent) ? `http://${MEDIA_SCHEME}.localhost/${key}` : `${MEDIA_SCHEME}://localhost/${key}`;
}

/** 첨부 id → 화면에서 쓰는 주소. 주소를 만드는 곳은 여기 한 군데다. */
export function mediaUrl(id: string): string {
  return mediaUrlFor(id, typeof navigator === 'undefined' ? '' : navigator.userAgent);
}
