const wait = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

function click(selector: string, index = 0) {
  document.querySelectorAll<HTMLElement>(selector)[index]?.click();
}

export async function demo() {
  const params = new URLSearchParams(location.search);
  const range = params.get("range");
  if (range !== null) click("#range button", Number(range));
  if (params.get("check") !== null) {
    click("#btn-check");
    await wait(2400);
  }
  if (params.get("solve") !== null) click(".solve-toggle", Number(params.get("solve")));
  if (params.get("app") !== null) {
    await wait(300);
    click(".app-row", Number(params.get("app")));
  }
  if (params.get("flow") !== null) {
    await wait(300);
    click(".flow-row", Number(params.get("flow")));
  }
  if (params.get("speed") !== null) {
    click("#btn-speed");
  }
}
