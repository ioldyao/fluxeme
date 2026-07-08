import { Routes, Route } from 'react-router-dom';
import { ProtectedRoute } from '@/components/ProtectedRoute';
import { AdminRoute } from '@/components/AdminRoute';
import { Layout } from '@/components/Layout';
import Login from '@/pages/Login';
import Register from '@/pages/Register';
import Dashboard from '@/pages/Dashboard';
import Users from '@/pages/Users';
import Channels from '@/pages/Channels';
import Models from '@/pages/Models';
import ModelsMarketplace from '@/pages/ModelsMarketplace';
import MyModels from '@/pages/MyModels';
import ApiKeys from '@/pages/ApiKeys';
import Rules from '@/pages/Rules';
import Usage from '@/pages/Usage';
import Profile from '@/pages/Profile';
import NotFound from '@/pages/NotFound';

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<Login />} />
      <Route path="/register" element={<Register />} />
      <Route element={<ProtectedRoute />}>
        <Route element={<Layout />}>
          <Route index element={<Dashboard />} />
          <Route element={<AdminRoute />}>
            <Route path="/users" element={<Users />} />
            <Route path="/channels" element={<Channels />} />
            <Route path="/models" element={<Models />} />
            <Route path="/rules" element={<Rules />} />
          </Route>
          <Route path="/models/marketplace" element={<ModelsMarketplace />} />
          <Route path="/models/mine" element={<MyModels />} />
          <Route path="/api-keys" element={<ApiKeys />} />
          <Route path="/usage" element={<Usage />} />
          <Route path="/profile" element={<Profile />} />
        </Route>
      </Route>
      <Route path="*" element={<NotFound />} />
    </Routes>
  );
}
