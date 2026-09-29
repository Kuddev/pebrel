return {
    summary = function(ctx, args)
        local runtime = ctx.runtime.call('runtime.describe', {})
        return {
            label = args.label or 'Pebrel',
            app_version = runtime.app_version,
            capability_count = #runtime.capabilities,
        }
    end,
}
