import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <div style={{ padding: 24, fontFamily: 'sans-serif' }}>
      <h1>admin-app scaffold OK</h1>
      <p>Port 5174 · replace with real App.tsx / routes in the next step.</p>
    </div>
  </StrictMode>,
);
