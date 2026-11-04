import { Component, Fragment, type ErrorInfo, type ReactNode } from 'react';
import { reportError } from './errorLog';
import SupportExport from './SupportExport';

declare global {
  interface Window {
    __crashDisarmed?: boolean;
  }
}

/** 디버그 빌드에서만: 주소에 `crash=1`이 있으면 처음 한 번 일부러 렌더링 오류를 낸다(오류 경계 확인용). */
function CrashProbe(): null {
  if (import.meta.env.DEV && !window.__crashDisarmed && /[?&]crash=1(?:&|$)/.test(window.location.search + window.location.hash)) {
    throw new Error('디버그용으로 일부러 낸 오류입니다');
  }
  return null;
}

interface Props {
  /** 기록에 남길 화면 이름 */
  name: string;
  children: ReactNode;
}

interface State {
  error: Error | null;
  attempt: number;
}

/** 렌더링 오류가 나면 흰 화면 대신 안내를 보여 준다. 「다시 시도」는 이 화면만 다시 그린다. */
export default class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null, attempt: 0 };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    const where = (info.componentStack ?? '').trim().split('\n')[0]?.trim() ?? '';
    reportError(`화면 오류(${this.props.name}): ${error.message}`, where);
  }

  retry = () => {
    window.__crashDisarmed = true;
    this.setState((s) => ({ error: null, attempt: s.attempt + 1 }));
  };

  render() {
    const { error, attempt } = this.state;
    if (error) {
      return (
        <main className="diag error-boundary" role="alert">
          <section className="diag-section">
            <h2>이 화면에 문제가 생겼습니다.</h2>
            <p>기록은 안전합니다. 「다시 시도」를 눌러 보세요. 계속되면 진단 파일을 만들어 문의해 주세요.</p>
            <button className="diag-primary" onClick={this.retry}>
              다시 시도
            </button>
            <details className="error-details">
              <summary>자세히</summary>
              <p className="diag-error">{error.message}</p>
            </details>
            <h3>문의하기</h3>
            <SupportExport />
          </section>
        </main>
      );
    }
    return (
      <Fragment key={attempt}>
        <CrashProbe />
        {this.props.children}
      </Fragment>
    );
  }
}
