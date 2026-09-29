import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import { Component } from "react";
import { Button } from "@/components/ui/button";
import { problemUrl } from "@/lib/errors";
/**
 * Catches render errors, most importantly a lazily loaded chunk that no longer exists
 * because a new version was deployed while this tab was open.
 */
export class ErrorBoundary extends Component {
    state = { error: null };
    static getDerivedStateFromError(error) {
        return { error };
    }
    render() {
        const { error } = this.state;
        if (!error)
            return this.props.children;
        const stale = /dynamically imported module|Loading chunk|Failed to fetch/i.test(error.message);
        return (_jsx("main", { className: "paper flex h-dvh items-center justify-center overflow-hidden p-6", role: "alert", children: _jsxs("div", { className: "panel flex max-w-md flex-col gap-3 p-6", children: [_jsx("div", { className: "text-[16px] font-semibold", children: stale ? "obj2cad was updated" : "Something went wrong" }), _jsx("p", { className: "m-0 text-[13.5px] text-fg-2", children: "Reload the page." }), !stale && _jsx("pre", { className: "num m-0 rounded-[3px] bg-panel-2 p-3 text-[12px] whitespace-pre-wrap text-fg-3", children: error.message }), _jsxs("div", { className: "flex items-center gap-4", children: [_jsx(Button, { variant: "primary", onClick: () => location.reload(), children: "Reload" }), !stale && (_jsx(Button, { variant: "link", asChild: true, children: _jsx("a", { href: problemUrl("Something went wrong", `${error.message}\n${error.stack ?? ""}`), target: "_blank", rel: "noreferrer", children: "Report a problem" }) }))] })] }) }));
    }
}
