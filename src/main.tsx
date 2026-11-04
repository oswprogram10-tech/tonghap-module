import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import BoardApp from './board/BoardApp';
import ErrorBoundary from './support/ErrorBoundary';
import { installErrorLogging } from './support/errorLog';
import './styles.css';

// 칠판 창은 같은 번들을 `#/board`로 연다.
const isBoard = window.location.hash.startsWith('#/board');

installErrorLogging();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    {isBoard ? (
      <ErrorBoundary name="칠판">
        <BoardApp />
      </ErrorBoundary>
    ) : (
      <App />
    )}
  </StrictMode>,
);
