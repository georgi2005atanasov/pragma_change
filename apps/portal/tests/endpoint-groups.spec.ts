import { test, expect } from "@playwright/test";
import { mkdirSync, readFileSync } from "node:fs";

test("new pipelines select one endpoint while saved groups remain readable", async ({
  page,
}) => {
  const endpoint = (id: string, path: string) => ({
    id,
    api_id: "api",
    name: path,
    method: "POST",
    path_template: path,
  });
  const endpoints = [
    endpoint("a", "/orders"),
    endpoint("b", "/payments"),
    endpoint("c", "/login"),
  ];
  const oldConfig = {
    ...JSON.parse(readFileSync("../../examples/pipeline.json", "utf8")),
    endpoint_id: "a",
    name: "Saved group",
    additional_endpoints: [
      {
        endpoint_id: "b",
        api_host: "api.example.test",
        method: "POST",
        path_template: "/payments",
      },
    ],
  };
  const oldPipeline = {
    id: "legacy-group",
    version: 1,
    schema_hash: "old-group-hash",
    configuration: oldConfig,
  };
  const model = (id: string, config: any) => ({
    id,
    endpoint_id: config.endpoint_id,
    pipeline_version_id: "legacy-group",
    pipeline_version: 1,
    pipeline_configuration: config,
    dataset_name: "Representative traffic",
    api_host: "api.example.test",
  });
  const data: Record<string, any> = {
    apis: [{ id: "api", name: "Example", host: "api.example.test" }],
    endpoints,
    pipelines: [oldPipeline],
    datasets: [],
    releases: [],
    "training-jobs": [],
    "audit-events": [],
    overview: {},
    models: [
      model("group", oldConfig),
      model("legacy", {
        ...oldConfig,
        name: "Payments only",
        endpoint_id: "b",
        path_template: "/payments",
        additional_endpoints: [],
      }),
      model("unrelated", {
        ...oldConfig,
        name: "Login only",
        endpoint_id: "c",
        path_template: "/login",
        additional_endpoints: [],
      }),
    ],
  };
  let saved: any;
  let release: any;
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname.slice(5);
    let response: any = data[path] || {};
    if (path === "pipelines" && route.request().method() === "POST") {
      saved = route.request().postDataJSON();
      response = {
        id: "pipeline",
        version: 1,
        schema_hash: "group-hash",
        configuration: saved,
      };
      data.pipelines = [response, oldPipeline];
    } else if (path === "pipelines/legacy-group/template") {
      response = {
        notes: [],
        request_events: [
          { endpoint_id: "a", method: "POST", path: "/orders" },
          { endpoint_id: "b", method: "POST", path: "/payments" },
        ],
      };
    } else if (path === "releases" && route.request().method() === "POST") {
      release = route.request().postDataJSON();
      response = { id: "release" };
    }
    await route.fulfill({ json: response });
  });
  await page.goto("/");
  await page
    .getByRole("button", { name: "Feature pipelines", exact: true })
    .click();
  await expect(page.getByLabel("Endpoint", { exact: true })).toBeVisible();
  await expect(
    page.getByLabel("Activity history", { exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("group", {
      name: "Endpoints sharing this pipeline and model",
    }),
  ).toHaveCount(0);
  await page.getByLabel("Endpoint", { exact: true }).selectOption("a");
  await page.getByLabel("Endpoint", { exact: true }).selectOption("b");
  await page.getByLabel("Pipeline name (optional)").fill("Payment activity");
  expect(
    JSON.parse(await page.getByLabel("Request JSON").innerText()).endpoint_id,
  ).toBe("b");
  await page.getByRole("tab", { name: "Pipeline JSON" }).click();
  const preview = JSON.parse(
    await page.locator('pre[aria-label="Pipeline JSON"]').innerText(),
  );
  expect(preview.endpoint_id).toBe("b");
  expect(preview.additional_endpoints || []).toEqual([]);
  mkdirSync("../../build/screenshots", { recursive: true });
  await page.screenshot({
    path: "../../build/screenshots/single-endpoint-desktop.png",
    fullPage: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth))
    .toBeLessThanOrEqual(390);
  await page.screenshot({
    path: "../../build/screenshots/single-endpoint-mobile.png",
    fullPage: true,
  });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByRole("button", { name: "Validate & save version →" }).click();
  await expect(
    page.getByText("New pipeline version created.", { exact: false }),
  ).toBeVisible();
  expect(saved.endpoint_id).toBe("b");
  expect(saved.additional_endpoints || []).toEqual([]);
  await page
    .locator("#resource-legacy-group")
    .getByRole("button", { name: "Use as draft" })
    .click();
  await expect(
    page.getByText("This draft comes from a previously saved endpoint group.", {
      exact: false,
    }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Validate & save version →" }),
  ).toBeDisabled();
  await expect(page.getByLabel("Endpoint", { exact: true })).toHaveValue("");
  await page.getByLabel("Endpoint", { exact: true }).selectOption("a");
  await expect(
    page.getByRole("button", { name: "Validate & save version →" }),
  ).toBeEnabled();
  await page.getByRole("button", { name: "Validate & save version →" }).click();
  await expect.poll(() => saved.endpoint_id).toBe("a");
  expect(saved.additional_endpoints || []).toEqual([]);
  expect(oldConfig.additional_endpoints).toHaveLength(1);
  await page
    .getByRole("button", { name: "Request replay", exact: true })
    .click();
  await page
    .getByLabel("Pipeline version", { exact: true })
    .selectOption("legacy-group");
  await page.getByRole("button", { name: "Load request template" }).click();
  await expect
    .poll(
      async () =>
        (
          await page
            .getByLabel("Timestamped request events (one JSON object per line)")
            .inputValue()
        ).split("\n").length,
    )
    .toBe(2);
  await page.getByRole("button", { name: "Releases", exact: true }).click();
  const shared = page.getByRole("checkbox", { name: /Saved group/ });
  const legacy = page.getByRole("checkbox", { name: /Payments only/ });
  const unrelated = page.getByRole("checkbox", { name: /Login only/ });
  await shared.check();
  await unrelated.check();
  await legacy.check();
  await expect(shared).not.toBeChecked();
  await expect(unrelated).toBeChecked();
  await shared.check();
  await expect(legacy).not.toBeChecked();
  for (const row of await page.locator(".release-choice").all()) {
    if (await row.getByRole("checkbox").isChecked()) {
      await row.getByLabel("Check threshold").fill("0.7");
      await row.getByLabel("Block threshold").fill("0.9");
    }
  }
  await page.getByLabel("Release name").fill("Grouped release");
  await page
    .getByRole("button", { name: "Validate & create release →" })
    .click();
  await expect(
    page.getByText("Release validated.", { exact: false }),
  ).toBeVisible();
  expect(release.models.map((m: any) => m.model_version_id).sort()).toEqual([
    "group",
    "unrelated",
  ]);
});
