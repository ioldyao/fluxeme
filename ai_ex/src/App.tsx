import { Routes, Route, Navigate } from 'react-router-dom'
import { AppLayout } from '@/components/layout/app-layout'
import DashboardPage from '@/pages/dashboard'
import ModelsPage from '@/pages/models'
import PlaygroundPage from '@/pages/playground'
import ApiKeysPage from '@/pages/api-keys'
import DocsPage from '@/pages/docs'

export default function App() {
  return (
    <Routes>
      <Route element={<AppLayout />}>
        <Route index element={<DashboardPage />} />
        <Route path="models" element={<ModelsPage />} />
        <Route path="playground" element={<PlaygroundPage />} />
        <Route path="keys" element={<ApiKeysPage />} />
        <Route path="docs" element={<DocsPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  )
}
