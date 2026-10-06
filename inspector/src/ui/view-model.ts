import type { ViewDescriptor } from '../domain';

export function groupViews(views: readonly ViewDescriptor[]): Map<string, ViewDescriptor[]> {
  const groups = new Map<string, ViewDescriptor[]>();
  for (const descriptor of views) {
    const entries = groups.get(descriptor.group) ?? [];
    entries.push(descriptor);
    groups.set(descriptor.group, entries);
  }
  return groups;
}
