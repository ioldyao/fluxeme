import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Bell, Plus, Pencil, Trash2, Check, X, Calendar } from 'lucide-react';
import { useAnnouncements, useCreateAnnouncement, useUpdateAnnouncement, useDeleteAnnouncement } from '@/api/announcements';
import type { Announcement } from '@/api/announcements';
import { PageHeader } from '@/components/PageHeader';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';

export default function AnnouncementsPage() {
  const { t } = useTranslation();
  const { data: items, isLoading } = useAnnouncements();
  const createAnnouncement = useCreateAnnouncement();
  const updateAnnouncement = useUpdateAnnouncement();
  const deleteAnnouncement = useDeleteAnnouncement();

  const [editing, setEditing] = useState<Announcement | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  const [title, setTitle] = useState('');
  const [content, setContent] = useState('');
  const [published, setPublished] = useState(false);

  const resetForm = () => {
    setTitle('');
    setContent('');
    setPublished(false);
    setEditing(null);
    setShowCreate(false);
  };

  const handleCreate = () => {
    if (!title.trim()) { toast.error(t('announcements.titleRequired')); return; }
    createAnnouncement.mutate({ title, content, published }, {
      onSuccess: () => { toast.success(t('announcements.created')); resetForm(); },
      onError: (err) => toast.error(err.message),
    });
  };

  const handleUpdate = () => {
    if (!editing || !title.trim()) return;
    updateAnnouncement.mutate({ id: editing.id, title, content, published }, {
      onSuccess: () => { toast.success(t('announcements.updated')); resetForm(); },
      onError: (err) => toast.error(err.message),
    });
  };

  const handleDelete = (item: Announcement) => {
    if (!window.confirm(t('announcements.deleteConfirm'))) return;
    deleteAnnouncement.mutate(item.id, {
      onSuccess: () => toast.success(t('announcements.deleted')),
      onError: (err) => toast.error(err.message),
    });
  };

  const openEdit = (item: Announcement) => {
    setEditing(item);
    setTitle(item.title);
    setContent(item.content);
    setPublished(item.published);
    setShowCreate(true);
  };

  return (
    <div className="space-y-6 animate-fade-in">
      <PageHeader
        title={t('announcements.title')}
        description={t('announcements.subtitle')}
        actions={
          <Button variant="default" size="sm" onClick={() => { resetForm(); setShowCreate(true); }}>
            <Plus className="size-4 mr-1" />{t('announcements.create')}
          </Button>
        }
      />

      {isLoading ? (
        <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
      ) : items && items.length > 0 ? (
        <div className="space-y-3">
          {items.map((item) => (
            <div key={item.id} className="rounded-xl border p-5">
              <div className="flex items-start justify-between gap-4">
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2 mb-1">
                    <h3 className="font-semibold">{item.title}</h3>
                    {item.published ? (
                      <span className="text-xs px-2 py-0.5 rounded-full bg-green-500/10 text-green-600 flex items-center gap-1">
                        <Check className="size-3" />{t('announcements.published')}
                      </span>
                    ) : (
                      <span className="text-xs px-2 py-0.5 rounded-full bg-muted text-muted-foreground flex items-center gap-1">
                        <X className="size-3" />{t('announcements.draft')}
                      </span>
                    )}
                  </div>
                  <p className="text-sm text-muted-foreground whitespace-pre-wrap">{item.content}</p>
                  <div className="flex items-center gap-3 mt-2 text-xs text-muted-foreground">
                    <span className="flex items-center gap-1"><Calendar className="size-3" />{new Date(item.created_at).toLocaleDateString()}</span>
                  </div>
                </div>
                <div className="flex items-center gap-1 shrink-0">
                  <Button variant="ghost" size="sm" onClick={() => openEdit(item)}>
                    <Pencil className="size-4" />
                  </Button>
                  <Button variant="ghost" size="sm" onClick={() => handleDelete(item)} className="text-destructive hover:text-destructive">
                    <Trash2 className="size-4" />
                  </Button>
                </div>
              </div>
            </div>
          ))}
        </div>
      ) : (
        <div className="p-12 text-center text-muted-foreground rounded-xl border">
          <Bell className="size-8 mx-auto mb-2 opacity-40" />
          <p>{t('announcements.empty')}</p>
        </div>
      )}

      {/* Create / Edit dialog */}
      <Dialog open={showCreate} onOpenChange={(open) => { if (!open) resetForm(); }}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>{editing ? t('announcements.edit') : t('announcements.create')}</DialogTitle>
          </DialogHeader>
          <div className="space-y-4">
            <div>
              <label className="text-xs text-muted-foreground mb-1 block">{t('announcements.titleLabel')}</label>
              <input
                type="text"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                className="h-9 w-full rounded-md border bg-background px-3 text-sm"
                placeholder={t('announcements.titlePlaceholder')}
              />
            </div>
            <div>
              <label className="text-xs text-muted-foreground mb-1 block">{t('announcements.contentLabel')}</label>
              <textarea
                value={content}
                onChange={(e) => setContent(e.target.value)}
                className="w-full rounded-md border bg-background px-3 py-2 text-sm min-h-[120px]"
                placeholder={t('announcements.contentPlaceholder')}
              />
            </div>
            <label className="flex items-center gap-2 text-sm cursor-pointer">
              <input
                type="checkbox"
                checked={published}
                onChange={(e) => setPublished(e.target.checked)}
                className="rounded"
              />
              {t('announcements.publishLabel')}
            </label>
            <div className="flex justify-end gap-2">
              <Button variant="outline" size="sm" onClick={resetForm}>{t('common.cancel')}</Button>
              <Button
                variant="default"
                size="sm"
                onClick={editing ? handleUpdate : handleCreate}
                disabled={createAnnouncement.isPending || updateAnnouncement.isPending || !title.trim()}
              >
                {editing ? t('announcements.save') : t('announcements.create')}
              </Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
