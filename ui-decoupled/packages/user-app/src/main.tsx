import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <div style={{ padding: 24, fontFamily: 'sans-serif' }}>
      <h1>user-app scaffold OK</h1>
      <p>Port 5173 · shared package verified. Real App.tsx / routes wired up in the next step.</p>
    </div>
  </StrictMode>,
);
