import { Component, type ErrorInfo, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/i18n";

function Failure({ retry }: { retry: () => void }) {
  const { t } = useI18n();
  return (
    <div role="alert" className="mx-auto my-8 max-w-lg rounded-2xl border bg-card p-6 text-center">
      <h2 className="text-base font-semibold">{t("error.title")}</h2>
      <p className="mt-2 text-sm text-muted-foreground">{t("error.description")}</p>
      <Button className="mt-4" onClick={retry}>{t("error.retry")}</Button>
    </div>
  );
}

/** 将渲染故障限制在当前模块；重试会重新挂载该模块。 */
export class ErrorBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("UI rendering failed", error, info.componentStack);
  }
  render() {
    return this.state.failed
      ? <Failure retry={() => this.setState({ failed: false })} />
      : this.props.children;
  }
}
