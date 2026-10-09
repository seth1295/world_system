import type { ViewCatalog, ViewDescriptor, ViewGroupDescriptor } from '../provider/contracts';

export interface OrderedViewGroup {
  group: ViewGroupDescriptor;
  views: readonly ViewDescriptor[];
}

/** Preserve provider group order and labels while indexing declared descriptors for display. */
export function orderedGroups(catalog: ViewCatalog): OrderedViewGroup[] {
  return [...catalog.groups]
    .sort((left, right) => left.order - right.order)
    .map((group) => ({ group, views: catalog.views.filter((view) => view.groupId === group.id) }))
    .filter(({ views }) => views.length > 0);
}
