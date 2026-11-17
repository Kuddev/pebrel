#import <Cocoa/Cocoa.h>
#import <ApplicationServices/ApplicationServices.h>

static id attribute(AXUIElementRef element, CFStringRef name) {
    CFTypeRef value = NULL;
    if (AXUIElementCopyAttributeValue(element, name, &value) != kAXErrorSuccess) return nil;
    return CFBridgingRelease(value);
}

static NSArray *children(AXUIElementRef element) {
    id value = attribute(element, kAXChildrenAttribute);
    return [value isKindOfClass:NSArray.class] ? value : @[];
}

static NSString *label(AXUIElementRef element) {
    NSMutableArray *parts = [NSMutableArray array];
    for (NSString *name in @[@"AXTitle", @"AXDescription", @"AXValue"]) {
        id value = attribute(element, (__bridge CFStringRef)name);
        if ([value isKindOfClass:NSString.class]) [parts addObject:value];
    }
    return [parts componentsJoinedByString:@" | "];
}

static NSDictionary *tree(AXUIElementRef element, NSString *path, int depth, int *budget) {
    if (--*budget < 0 || depth > 18) return @{@"truncated": @YES, @"path": path};
    AXUIElementSetMessagingTimeout(element, 0.25);
    id role = attribute(element, kAXRoleAttribute);
    if (![role isKindOfClass:NSString.class]) return @{@"unavailable": @YES, @"path": path};
    NSMutableDictionary *node = [NSMutableDictionary dictionaryWithDictionary:@{@"path": path, @"AXRole": role}];
    for (NSString *name in @[@"AXSubrole", @"AXTitle", @"AXDescription", @"AXValue", @"AXIdentifier"]) {
        id value = attribute(element, (__bridge CFStringRef)name);
        if ([value isKindOfClass:NSString.class] || [value isKindOfClass:NSNumber.class]) node[name] = value;
    }
    CFArrayRef actions = NULL;
    if (AXUIElementCopyActionNames(element, &actions) == kAXErrorSuccess)
        node[@"actions"] = CFBridgingRelease(actions);
    id position = attribute(element, kAXPositionAttribute);
    id size = attribute(element, kAXSizeAttribute);
    CGPoint point; CGSize extent;
    if (position && size && CFGetTypeID((__bridge CFTypeRef)position) == AXValueGetTypeID()
        && CFGetTypeID((__bridge CFTypeRef)size) == AXValueGetTypeID()
        && AXValueGetValue((__bridge AXValueRef)position, kAXValueCGPointType, &point)
        && AXValueGetValue((__bridge AXValueRef)size, kAXValueCGSizeType, &extent)) {
        node[@"rect"] = @[@(point.x), @(point.y), @(extent.width), @(extent.height)];
    }
    NSMutableArray *descendants = [NSMutableArray array];
    NSArray *items = children(element);
    for (NSUInteger i = 0; i < items.count && *budget > 0; ++i) {
        NSString *childPath = path.length ? [path stringByAppendingFormat:@"/%lu", (unsigned long)i]
                                         : [NSString stringWithFormat:@"%lu", (unsigned long)i];
        [descendants addObject:tree((__bridge AXUIElementRef)items[i], childPath, depth + 1, budget)];
    }
    node[@"children"] = descendants;
    return node;
}

static void output(id value) {
    NSData *data = [NSJSONSerialization dataWithJSONObject:value options:NSJSONWritingPrettyPrinted error:nil];
    puts([[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding].UTF8String);
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        if (argc < 2 || !AXIsProcessTrusted()) return 2;
        NSString *command = @(argv[1]);
        if ([command isEqual:@"snapshot"]) {
            NSMutableArray *apps = [NSMutableArray array];
            for (NSRunningApplication *app in NSWorkspace.sharedWorkspace.runningApplications) {
                NSString *name = app.localizedName ?: @"";
                NSString *bundle = app.bundleIdentifier ?: @"";
                BOOL product = [bundle isEqual:@"io.github.kuddev.pebrel.preview"]
                    || [bundle isEqual:@"io.github.kuddev.pebrel"];
                if (product) {
                    // Product focus is observed through NSWorkspace and its Runtime API.
                    // Traversing its menu bar is unrelated to native notification delivery.
                    [apps addObject:@{@"name":name, @"bundle":bundle, @"pid":@(app.processIdentifier),
                                      @"active":@(app.active), @"tree":@{@"children":@[]}}];
                    continue;
                }
                if ([name isEqual:@"SoftwareUpdateNotificationManager"]) continue;
                if (![name containsString:@"Notification"] && ![bundle.lowercaseString containsString:@"notification"]
                    && ![name isEqual:@"Control Center"] && ![name isEqual:@"System Settings"]) continue;
                AXUIElementRef element = AXUIElementCreateApplication(app.processIdentifier);
                int budget = 600;
                [apps addObject:@{@"name":name, @"bundle":bundle, @"pid":@(app.processIdentifier),
                                  @"active":@(app.active), @"tree":tree(element, @"", 0, &budget)}];
                CFRelease(element);
            }
            output(@{@"apps":apps});
            return 0;
        }
        if (argc < 5) return 2;
        pid_t pid = (pid_t)strtol(argv[2], NULL, 10);
        AXUIElementRef root = AXUIElementCreateApplication(pid);
        AXUIElementRef element = root;
        NSMutableArray *retained = [NSMutableArray array];
        NSString *path = @(argv[3]);
        for (NSString *part in [path componentsSeparatedByString:@"/"]) {
            if (!part.length) continue;
            NSArray *items = children(element);
            NSUInteger index = part.integerValue;
            if (index >= items.count) { CFRelease(root); return 3; }
            [retained addObject:items];
            element = (__bridge AXUIElementRef)items[index];
        }
        NSString *expected = @(argv[4]);
        if (![label(element) containsString:expected]) { CFRelease(root); return 4; }
        int status = 0;
        if ([command isEqual:@"press"]) {
            status = AXUIElementPerformAction(element, kAXPressAction) == kAXErrorSuccess ? 0 : 5;
        } else if ([command isEqual:@"action"] && argc == 6) {
            CFArrayRef available = NULL;
            NSString *requested = @(argv[5]);
            if (AXUIElementCopyActionNames(element, &available) != kAXErrorSuccess) {
                CFRelease(root); return 7;
            }
            NSArray *names = CFBridgingRelease(available);
            if (![names containsObject:requested]) { CFRelease(root); return 8; }
            status = AXUIElementPerformAction(element, (__bridge CFStringRef)requested) == kAXErrorSuccess ? 0 : 5;
        } else if ([command isEqual:@"click"]) {
            CGPoint point; CGSize size;
            id position = attribute(element, kAXPositionAttribute);
            id extent = attribute(element, kAXSizeAttribute);
            if (!position || !extent
                || !AXValueGetValue((__bridge AXValueRef)position, kAXValueCGPointType, &point)
                || !AXValueGetValue((__bridge AXValueRef)extent, kAXValueCGSizeType, &size)
                || size.width <= 0 || size.height <= 0) { CFRelease(root); return 6; }
            point.x += size.width / 2; point.y += size.height / 2;
            CGEventRef down = CGEventCreateMouseEvent(NULL, kCGEventLeftMouseDown, point, kCGMouseButtonLeft);
            CGEventRef up = CGEventCreateMouseEvent(NULL, kCGEventLeftMouseUp, point, kCGMouseButtonLeft);
            CGEventPost(kCGHIDEventTap, down); CGEventPost(kCGHIDEventTap, up);
            CFRelease(down); CFRelease(up);
        } else status = 2;
        CFRelease(root);
        return status;
    }
}
