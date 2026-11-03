import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import BoardApp from './board/BoardApp';
import './styles.css';

// 칠판 창은 같은 번들을 `#/board`로 연다.
const isBoard = window.location.hash.startsWith('#/board');

createRoot(document.getElementById('root')!).render(<StrictMode>{isBoard ? <BoardApp /> : <App />}</StrictMode>);
