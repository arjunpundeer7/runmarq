import { ContentPage } from "@/components/Prose";
import { ButtonLink } from "@/components/site";

export default function NotFound() {
  return (
    <ContentPage>
      <div className="max-w-xl py-16">
        <h1 className="text-[clamp(2.375rem,1.3rem+3.6vw,4.5rem)] leading-[1] tracking-[-0.035em]">This page does not exist.</h1>
        <div className="mt-9 flex flex-wrap gap-3">
          <ButtonLink href="/">Home</ButtonLink>
          <ButtonLink href="/docs" variant="ghost" arrow={false}>
            Documentation
          </ButtonLink>
        </div>
      </div>
    </ContentPage>
  );
}
