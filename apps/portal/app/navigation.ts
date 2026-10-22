/** Stable public section identifiers. Labels and order can change independently of URLs. */
export const sectionDefinitions = [
  {
    id: "overview",
    label: "Overview",
    caption: "Your behavioral security workspace",
  },
  {
    id: "endpoints",
    label: "Endpoints",
    caption: "Define the surface you want to protect",
  },
  {
    id: "feature-pipelines",
    label: "Feature pipelines",
    caption: "Turn request history into a consistent signal",
  },
  {
    id: "datasets",
    label: "Datasets",
    caption: "Give the model a representative baseline",
  },
  {
    id: "training-models",
    label: "Training & models",
    caption: "Learn what normal looks like",
  },
  {
    id: "request-replay",
    label: "Request replay",
    caption: "Follow a request through every step",
  },
  {
    id: "releases",
    label: "Releases",
    caption: "Bring your model to the proxy",
  },
  {
    id: "audit-log",
    label: "Audit log",
    caption: "A record of administrative changes",
  },
  {
    id: "servers",
    label: "Servers",
    caption: "Connect, inspect, and deploy to remote Nginx hosts",
  },
  {
    id: "behavior",
    label: "Behavior",
    caption: "Collect application events and review unusual journeys",
  },
  {
    id: "assisted-setup",
    label: "Assisted setup",
    caption: "Prepare configurations with Claude and review their evidence",
  },
] as const;
export const sections = sectionDefinitions.map((section) => section.label);
export const captions = sectionDefinitions.map((section) => section.caption);
export function sectionIndex(id: string | null): number {
  return Math.max(
    0,
    sectionDefinitions.findIndex((section) => section.id === id),
  );
}
/** Only known section identifiers cross the sign-in boundary, never arbitrary redirect URLs. */
export function sectionHref(
  index: number,
  pathname: "/" | "/login" = "/",
): string {
  const section = sectionDefinitions[index];
  return !section || index === 0
    ? pathname
    : `${pathname}?section=${section.id}`;
}
