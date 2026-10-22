import { test, expect } from "@playwright/test";

const appId = "22222222-2222-4222-8222-222222222222";
const pipelineId = "11111111-1111-4111-8111-111111111111";
const hash = "a".repeat(64);
const file = (name: string, value: string) => ({
  name,
  mimeType: "application/json",
  buffer: Buffer.from(value),
});

test("offline recordings reach the correct import with their manifest intact", async ({
  page,
}) => {
  const mutations: { path: string; body: any }[] = [];
  await page.route("**/api/**", async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname.slice(5);
    if (request.method() === "POST")
      mutations.push({ path, body: request.postDataJSON() });
    const data =
      path === "overview"
        ? {}
        : path === "behavior/apps"
          ? [
              {
                id: appId,
                name: "Test application",
                policy: { enforce: false },
                collected_events: 0,
              },
            ]
          : path === "pipelines"
            ? [
                {
                  id: pipelineId,
                  version: 1,
                  configuration: {
                    name: "Orders",
                    method: "POST",
                    path_template: "/orders",
                    features: [],
                  },
                },
              ]
            : path === "datasets/preview"
              ? {
                  source_rows: 40,
                  warmup_rows: 10,
                  training_rows: 30,
                  errors: [],
                  feature_names: ["body_bytes"],
                  preview: [[900]],
                  valid: true,
                }
              : [];
    await route.fulfill({
      json:
        request.method() === "POST" && path !== "datasets/preview"
          ? { id: "snapshot" }
          : data,
    });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Behavior", exact: true }).click();
  await page.getByLabel("Recording application").selectOption(appId);
  const behavior = {
    format_version: 1,
    finalized: true,
    recording_id: pipelineId,
    config: { format: "behavior_events", application_id: appId },
    row_count: 1,
    byte_count: 3,
    content_sha256: hash,
  };
  await page
    .getByLabel("Behavior manifest")
    .setInputFiles(file("manifest.json", JSON.stringify(behavior)));
  await page
    .getByLabel("Behavior records")
    .setInputFiles(file("records.jsonl", "{}\n"));
  await page
    .getByRole("button", { name: "Import recording", exact: true })
    .click();
  await expect
    .poll(
      () =>
        mutations.find((m) => m.path === `behavior/apps/${appId}/imports`)
          ?.body,
    )
    .toEqual({ manifest: behavior, content: "{}\n" });

  await page.getByRole("button", { name: "Datasets", exact: true }).click();
  const requestManifest = {
    ...behavior,
    pipeline_version_id: pipelineId,
    schema_hash: hash,
    row_count: 40,
    config: {
      format: "request_events",
      pipeline_version_id: pipelineId,
      schema_hash: hash,
      endpoint_id: appId,
    },
  };
  await page
    .getByLabel("SDK recording manifest (optional)")
    .setInputFiles(file("manifest.json", JSON.stringify(requestManifest)));
  await expect(page.getByLabel("Schema hash")).toHaveValue(hash);
  await page
    .getByLabel("Choose file", { exact: true })
    .setInputFiles(file("records.jsonl", "{}\n"));
  await page.getByLabel("Dataset name").fill("Offline orders");
  await page.getByRole("button", { name: "Preview & validate" }).click();
  await expect
    .poll(
      () =>
        mutations.find((m) => m.path === "datasets/preview")?.body
          .recording_manifest,
    )
    .toEqual(requestManifest);
  await page.getByRole("button", { name: "Accept import →" }).click();
  await expect
    .poll(
      () =>
        mutations.find((m) => m.path === "datasets")?.body.pipeline_version_id,
    )
    .toBe(pipelineId);
  await page.setViewportSize({ width: 390, height: 844 });
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
});
