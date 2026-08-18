import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import {
  listSsoConfigs,
  createSsoConfig,
  updateSsoConfig,
  deleteSsoConfig,
} from '@fluxeme/shared/src/api/settings';
import { EmptyState } from '@fluxeme/shared/src/components/EmptyState';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@fluxeme/shared/src/components/ui/dialog';
import { Input } from '@fluxeme/shared/src/components/ui/input';
import { Label } from '@fluxeme/shared/src/components/ui/label';
import { Switch } from '@fluxeme/shared/src/components/ui/switch';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@fluxeme/shared/src/components/ui/select';
import { Checkbox } from '@fluxeme/shared/src/components/ui/checkbox';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { Plus, Pencil, Trash2 } from 'lucide-react';
import type { SsoConfig, SsoConfigRequest } from '@fluxeme/shared/src/types';

const DEFAULT_FORM: SsoConfigRequest = {
  team_id: null,
  provider_name: '',
  issuer_url: '',
  client_id: '',
  client_secret: '',
  redirect_url: '',
  enabled: true,
  auto_create_user: true,
  domain_restrictions: null,
  default_role: 'user',
};

export default function SsoSettings() {
  const { t } = useTranslation();
  const [configs, setConfigs] = useState<SsoConfig[]>([]);
  const [loading, setLoading] = useState(true);
  const [showDialog, setShowDialog] = useState(false);
  const [editTarget, setEditTarget] = useState<SsoConfig | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<SsoConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [form, setForm] = useState<SsoConfigRequest>(DEFAULT_FORM);

  const load = async () => {
    setLoading(true);
    try {
      const data = await listSsoConfigs();
      setConfigs(data);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to load SSO configs');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { load(); }, []);

  const openAdd = () => {
    setEditTarget(null);
    setForm(DEFAULT_FORM);
    setShowDialog(true);
  };

  const openEdit = (cfg: SsoConfig) => {
    setEditTarget(cfg);
    setForm({
      team_id: cfg.team_id ?? null,
      provider_name: cfg.provider_name,
      issuer_url: cfg.issuer_url,
      client_id: cfg.client_id,
      client_secret: '',
      redirect_url: cfg.redirect_url,
      enabled: cfg.enabled,
      auto_create_user: cfg.auto_create_user,
      domain_restrictions: cfg.domain_restrictions ?? null,
      default_role: cfg.default_role,
    });
    setShowDialog(true);
  };

  const handleSave = async () => {
    if (!form.provider_name || !form.issuer_url || !form.client_id) {
      toast.error('Provider name, issuer URL, and client ID are required');
      return;
    }
    if (!editTarget && !form.client_secret) {
      toast.error('Client secret is required when creating a new SSO config');
      return;
    }

    setSaving(true);
    try {
      if (editTarget) {
        await updateSsoConfig(editTarget.id, form);
        toast.success(t('ssoSettings.updated'));
      } else {
        await createSsoConfig(form);
        toast.success(t('ssoSettings.created'));
      }
      setShowDialog(false);
      load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to save SSO configuration');
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!deleteTarget) return;
    try {
      await deleteSsoConfig(deleteTarget.id);
      toast.success(t('ssoSettings.deleted'));
      setDeleteTarget(null);
      load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to delete SSO configuration');
    }
  };

  const update = <K extends keyof SsoConfigRequest>(
    key: K,
    value: SsoConfigRequest[K],
  ) => {
    setForm((prev) => ({ ...prev, [key]: value }));
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-base font-semibold">{t('ssoSettings.title')}</h2>
          <p className="text-xs text-muted-foreground mt-0.5">{t('ssoSettings.subtitle')}</p>
        </div>
        <Button onClick={openAdd}>
          <Plus className="size-4 mr-1" />
          {t('ssoSettings.add')}
        </Button>
      </div>

      <Card>
        <CardContent className="p-0">
          {loading ? (
            <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
          ) : configs.length === 0 ? (
            <EmptyState
              message={t('ssoSettings.noConfigs')}
              action={
                <Button onClick={openAdd}>
                  <Plus className="size-4 mr-1" />
                  {t('ssoSettings.add')}
                </Button>
              }
            />
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground">
                    <th className="text-left py-3 px-4">{t('ssoSettings.providerName')}</th>
                    <th className="text-left py-3 px-4">Issuer URL</th>
                    <th className="text-center py-3 px-4">{t('ssoSettings.boundTeam')}</th>
                    <th className="text-center py-3 px-4">{t('ssoSettings.enabled')}</th>
                    <th className="text-right py-3 px-4">{t('ssoSettings.defaultRole')}</th>
                    <th className="text-right py-3 px-4">{t('table.actions')}</th>
                  </tr>
                </thead>
                <tbody>
                  {configs.map((cfg) => (
                    <tr key={cfg.id} className="border-b last:border-0 hover:bg-muted/50">
                      <td className="py-3 px-4 font-medium">{cfg.provider_name}</td>
                      <td className="py-3 px-4 text-xs text-muted-foreground truncate max-w-[200px]">
                        {cfg.issuer_url}
                      </td>
                      <td className="py-3 px-4 text-center text-xs text-muted-foreground">
                        {cfg.team_id || t('ssoSettings.global')}
                      </td>
                      <td className="py-3 px-4 text-center">
                        <Switch checked={cfg.enabled} onCheckedChange={() => {}} />
                      </td>
                      <td className="py-3 px-4 text-right text-xs">{cfg.default_role}</td>
                      <td className="py-3 px-4 text-right whitespace-nowrap">
                        <Button variant="ghost" size="sm" onClick={() => openEdit(cfg)}>
                          <Pencil className="size-3.5" />
                        </Button>
                        <Button variant="ghost" size="sm" onClick={() => setDeleteTarget(cfg)}>
                          <Trash2 className="size-3.5 text-destructive" />
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Add/Edit Dialog */}
      <Dialog open={showDialog} onOpenChange={setShowDialog}>
        <DialogContent className="sm:max-w-lg p-0 gap-0">
          <DialogHeader className="px-6 py-5 border-b shrink-0">
            <DialogTitle className="text-lg font-semibold">
              {editTarget ? t('ssoSettings.edit') : t('ssoSettings.add')}
            </DialogTitle>
          </DialogHeader>

          <form
            onSubmit={(e) => { e.preventDefault(); handleSave(); }}
            className="p-6 space-y-4"
          >
            <div className="space-y-2">
              <Label>{t('ssoSettings.providerName')}</Label>
              <Input
                value={form.provider_name}
                onChange={(e) => update('provider_name', e.target.value)}
                placeholder="e.g. Google, Azure AD"
                required
              />
            </div>

            <div className="space-y-2">
              <Label>{t('ssoSettings.issuerUrl')}</Label>
              <Input
                value={form.issuer_url}
                onChange={(e) => update('issuer_url', e.target.value)}
                placeholder="https://accounts.google.com"
                required
              />
            </div>

            <div className="space-y-2">
              <Label>{t('ssoSettings.clientId')}</Label>
              <Input
                value={form.client_id}
                onChange={(e) => update('client_id', e.target.value)}
                required
              />
            </div>

            <div className="space-y-2">
              <Label>{t('ssoSettings.clientSecret')}</Label>
              <Input
                type="password"
                value={form.client_secret}
                onChange={(e) => update('client_secret', e.target.value)}
                placeholder={editTarget ? 'Leave empty to keep current' : ''}
                required={!editTarget}
              />
            </div>

            <div className="space-y-2">
              <Label>{t('ssoSettings.redirectUrl')}</Label>
              <Input
                value={form.redirect_url}
                onChange={(e) => update('redirect_url', e.target.value)}
                placeholder="https://your-gateway/admin/api/sso/callback"
              />
            </div>

            <div className="space-y-2">
              <Label>{t('ssoSettings.domainRestrictions')}</Label>
              <Input
                value={form.domain_restrictions ?? ''}
                onChange={(e) => update('domain_restrictions', e.target.value || null)}
                placeholder="example.com, mycompany.org"
              />
              <p className="text-xs text-muted-foreground">
                {t('ssoSettings.domainRestrictionsHint')}
              </p>
            </div>

            <div className="space-y-2">
              <Label>{t('ssoSettings.defaultRole')}</Label>
              <Select
                value={form.default_role}
                onValueChange={(v) => update('default_role', v)}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="user">user</SelectItem>
                  <SelectItem value="admin">admin</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="grid grid-cols-2 gap-4 pt-2">
              <label className="flex items-center gap-2 text-sm">
                <Switch
                  checked={form.enabled}
                  onCheckedChange={(v) => update('enabled', v)}
                />
                <div>
                  <span>{t('ssoSettings.enabled')}</span>
                  <p className="text-xs text-muted-foreground">{t('ssoSettings.enabledHint')}</p>
                </div>
              </label>

              <label className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={form.auto_create_user}
                  onCheckedChange={(v) => update('auto_create_user', !!v)}
                />
                <span>{t('ssoSettings.autoCreateUser')}</span>
              </label>
            </div>

            <div className="flex justify-end gap-2 pt-2">
              <Button
                type="button"
                variant="outline"
                onClick={() => setShowDialog(false)}
              >
                {t('common.cancel')}
              </Button>
              <Button type="submit" disabled={saving}>
                {saving ? t('common.saving') : t('common.save')}
              </Button>
            </div>
          </form>
        </DialogContent>
      </Dialog>

      {/* Delete confirmation */}
      <ConfirmDialog
        open={!!deleteTarget}
        onOpenChange={(open) => { if (!open) setDeleteTarget(null); }}
        title={t('ssoSettings.confirmDelete')}
        description={t('ssoSettings.confirmDeleteDetail')}
        onConfirm={handleDelete}
      />
    </div>
  );
}
