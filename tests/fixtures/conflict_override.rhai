fn resolve(conflict) {
    if conflict.reason.contains("expected") {
        return #{ action: "override", message: "override via script" };
    }
    return #{ action: "abort", message: "script aborted conflict" };
}
