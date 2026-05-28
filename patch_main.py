import re

with open("apps/android/app/src/main/kotlin/com/xiwei/writerapp/ui/MainActivity.kt", "r") as f:
    content = f.read()

# 1. Add `private lateinit var fabNewStarMapNode: ExtendedFloatingActionButton`
content = re.sub(
    r'    private lateinit var fabNewProject: ExtendedFloatingActionButton\n',
    '    private lateinit var fabNewProject: ExtendedFloatingActionButton\n    private lateinit var fabNewStarMapNode: ExtendedFloatingActionButton\n',
    content, count=1
)

# 2. Add `fabNewStarMapNode = findViewById(R.id.fabNewStarMapNode)`
content = re.sub(
    r'        fabNewProject = findViewById\(R\.id\.fabNewProject\)\n',
    '        fabNewProject = findViewById(R.id.fabNewProject)\n        fabNewStarMapNode = findViewById(R.id.fabNewStarMapNode)\n',
    content, count=1
)

# 3. Add `fabNewStarMapNode.setOnClickListener { starMapController.showNewNodeDialog() }`
content = re.sub(
    r'        fabNewProject\.setOnClickListener \{\n            showNewProjectDialog\(\)\n        \}\n',
    '        fabNewProject.setOnClickListener {\n            showNewProjectDialog()\n        }\n\n        fabNewStarMapNode.setOnClickListener {\n            starMapController.showNewNodeDialog()\n        }\n',
    content, count=1
)

# 4. Hide/Show logic in `onCreate` initially
content = re.sub(
    r'                fabNewProject\.show\(\)\n',
    '                fabNewProject.show()\n                fabNewStarMapNode.hide()\n',
    content, count=1
)

# Replace the first `fabNewProject.hide()` inside `nav_starmap` (in onCreate)
content = re.sub(
    r'                toolbar\.title = "星图"\n                starMapController\.initialize\(starmapId\)\n                fabNewProject\.hide\(\)\n',
    '                toolbar.title = "星图"\n                starMapController.initialize(starmapId)\n                fabNewProject.hide()\n                fabNewStarMapNode.show()\n',
    content, count=1
)

# Replace the second `fabNewProject.hide()` inside `nav_stats` (in onCreate)
content = re.sub(
    r'                toolbar\.title = "统计"\n                statsController\.initialize\(\)\n                fabNewProject\.hide\(\)\n',
    '                toolbar.title = "统计"\n                statsController.initialize()\n                fabNewProject.hide()\n                fabNewStarMapNode.hide()\n',
    content, count=1
)

# 5. Hide/Show logic in `bottomNav.setOnItemSelectedListener`
# Same pattern, just replace them all
content = re.sub(
    r'                    toolbar\.title = "作品"\n                    fabNewProject\.show\(\)\n',
    '                    toolbar.title = "作品"\n                    fabNewProject.show()\n                    fabNewStarMapNode.hide()\n',
    content, count=1
)

content = re.sub(
    r'                    toolbar\.title = "星图"\n                    starMapController\.initialize\(starmapId\)\n                    fabNewProject\.hide\(\)\n',
    '                    toolbar.title = "星图"\n                    starMapController.initialize(starmapId)\n                    fabNewProject.hide()\n                    fabNewStarMapNode.show()\n',
    content, count=1
)

content = re.sub(
    r'                    toolbar\.title = "统计"\n                    statsController\.initialize\(\)\n                    fabNewProject\.hide\(\)\n',
    '                    toolbar.title = "统计"\n                    statsController.initialize()\n                    fabNewProject.hide()\n                    fabNewStarMapNode.hide()\n',
    content, count=1
)

with open("apps/android/app/src/main/kotlin/com/xiwei/writerapp/ui/MainActivity.kt", "w") as f:
    f.write(content)

