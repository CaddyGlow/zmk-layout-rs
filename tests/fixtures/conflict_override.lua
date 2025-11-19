function resolve(conflict)
    if string.find(conflict.reason, "expected") then
        return { action = "override", message = "override via script" }
    end
    return { action = "abort", message = "script aborted conflict" }
end
