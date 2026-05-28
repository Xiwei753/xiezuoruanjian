import re

with open("apps/android/app/src/main/res/layout/activity_settings.xml", "r") as f:
    content = f.read()

# We want to remove the whole section related to proxy.
# Looking for `<TextView android:text="代理类型"` to the end of the proxy group.
# Usually they are grouped in a `<LinearLayout>` or under a `<TextView>` and `<Spinner>`.

# Let's just find where it is first.
