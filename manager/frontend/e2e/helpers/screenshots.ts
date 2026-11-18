import path from 'node:path';
import { expect, type Page, test } from '@playwright/test';
import { FONT_STYLESHEET_ATTRIBUTE } from '../../src/shared/stylesheet';

const BODY_FONT = '16px Nunito';

export async function fontsLoaded(page: Page): Promise<void> {
  await page.waitForFunction(
    (attribute) =>
      Boolean(
        document.querySelector<HTMLLinkElement>(`link[rel="stylesheet"][${attribute}]`)?.sheet
      ),
    FONT_STYLESHEET_ATTRIBUTE
  );
  await page.evaluate(async (font) => {
    await document.fonts.load(font);
    await new Promise(requestAnimationFrame);
    await document.fonts.ready;
  }, BODY_FONT);
}

export async function expectNoSidewaysScroll(page: Page, scrollers: string): Promise<void> {
  const overflow = await page.evaluate(
    (selector) =>
      Math.max(
        document.documentElement.scrollWidth - document.documentElement.clientWidth,
        ...Array.from(
          document.querySelectorAll(selector),
          (element) => element.scrollWidth - element.clientWidth
        )
      ),
    scrollers
  );
  expect(overflow, 'the page never scrolls sideways').toBeLessThanOrEqual(0);
}

function contentHeight(page: Page): Promise<number> {
  return page.evaluate(() =>
    Math.ceil(
      Math.max(
        ...Array.from(
          document.querySelectorAll('*'),
          (element) => element.getBoundingClientRect().top + element.scrollHeight
        )
      )
    )
  );
}

function screenshotPath(directory: string | undefined, name: string): string {
  const file = `${name}.png`;
  return directory ? path.join(directory, file) : test.info().outputPath(file);
}

export async function shoot(
  page: Page,
  directory: string | undefined,
  name: string,
  scrollers: string
): Promise<void> {
  const viewport = page.viewportSize();
  if (!viewport) {
    throw new Error('a full-page screenshot needs a fixed viewport');
  }
  await fontsLoaded(page);
  await expectNoSidewaysScroll(page, scrollers);
  let height = viewport.height;
  for (let measured = await contentHeight(page); measured > height; ) {
    height = measured;
    await page.setViewportSize({ width: viewport.width, height });
    measured = await contentHeight(page);
  }
  await expectNoSidewaysScroll(page, scrollers);
  await page.screenshot({ path: screenshotPath(directory, name), animations: 'disabled' });
  await page.setViewportSize(viewport);
}
